//! 聚合后台用例：领取 jobs → decide_aggregation → 写 session_id → complete。

use crate::application::dto::{AggregateClaim, ChangeOrigin, ChangeSet};
use crate::application::error::AppError;
use crate::domain::entities::{Contact, Group, GroupParticipant, MessageWithRecipients};
use crate::domain::services::{
    decide_aggregation, resolve_self_addresses, AggregationContext, AggregationPlan, GroupTarget,
    ParentSessionHint, SessionTarget,
};
use crate::domain::value_objects::{
    AccountId, AggregateJobStatus, AggregationOutcome, ContactId, ContactSource, EmailAddress,
    GroupId, GroupSource, MessageSyncStatus, SessionId, SessionKind, TimestampMs,
};
use crate::infrastructure::repository::{
    accounts, contacts, groups, jobs, messages, sessions,
};
use crate::runtime::ChangeDispatcher;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const MAX_ATTEMPTS_PER_GENERATION: i32 = 5;
const LEASE_MS: i64 = 60_000;

fn now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn backoff_ms(attempts: i32) -> i64 {
    let exp = attempts.clamp(0, 6) as u32;
    (3_000i64).saturating_mul(1i64 << exp).min(10 * 60_000)
}

/// 处理至多 `limit` 条可用聚合任务；可选分发 ChangeSet。
pub async fn process_available_jobs(
    db: &SqlitePool,
    changes: Option<&ChangeDispatcher>,
    limit: u32,
) -> Result<u32, AppError> {
    if limit == 0 {
        return Ok(0);
    }
    let account_ids = {
        let mut conn = db.acquire().await?;
        let list = accounts::list(&mut conn).await.map_err(AppError::from)?;
        list.into_iter().map(|a| a.id).collect::<Vec<_>>()
    };

    let mut remaining = limit;
    let mut completed = 0u32;
    let mut changeset_acc: Option<ChangeSet> = None;

    for account_id in account_ids {
        if remaining == 0 {
            break;
        }
        {
            let mut conn = db.acquire().await?;
            jobs::release_expired_leases(&mut conn, &account_id, now_ms())
                .await
                .map_err(AppError::from)?;
        }

        let lease_token = Uuid::new_v4().to_string();
        let lease_expires_at = now_ms() + LEASE_MS;
        let claimed = {
            let mut conn = db.acquire().await?;
            jobs::claim_batch(
                &mut conn,
                &account_id,
                now_ms(),
                &lease_token,
                lease_expires_at,
                remaining,
            )
            .await
            .map_err(AppError::from)?
        };

        for job in claimed {
            remaining = remaining.saturating_sub(1);
            let claim = AggregateClaim {
                account_id: job.account_id.clone(),
                message_id: job.message_id.clone(),
                generation: job.generation,
                lease_token: lease_token.clone(),
            };
            match aggregate_message(db, &claim, job.attempts).await {
                Ok((outcome, cs)) => {
                    if outcome == AggregationOutcome::Completed
                        || outcome == AggregationOutcome::Partial
                        || outcome == AggregationOutcome::Skipped
                    {
                        completed += 1;
                    }
                    if let Some(cs) = cs {
                        match &mut changeset_acc {
                            Some(acc) => {
                                acc.message_ids.extend(cs.message_ids);
                                acc.session_ids.extend(cs.session_ids);
                                acc.new_incoming_ids.extend(cs.new_incoming_ids);
                            }
                            None => changeset_acc = Some(cs),
                        }
                    }
                }
                Err(e) => {
                    let _ = e;
                }
            }
        }
    }

    if let (Some(dispatcher), Some(mut cs)) = (changes, changeset_acc) {
        let mut seen = HashSet::new();
        cs.session_ids.retain(|c| seen.insert(c.clone()));
        if !cs.is_empty() {
            dispatcher.dispatch(cs);
        }
    }

    Ok(completed)
}

/// 单条任务：校验租约代次后决策并写投影。
pub async fn aggregate_message(
    db: &SqlitePool,
    claim: &AggregateClaim,
    attempts: i32,
) -> Result<(AggregationOutcome, Option<ChangeSet>), AppError> {
    let mut tx = db.begin().await?;

    let row = sqlx::query_as::<_, (i64, String, Option<String>)>(
        "SELECT generation, status, lease_token FROM session_aggregate_jobs
         WHERE account_id = ? AND message_id = ?",
    )
    .bind(claim.account_id.as_str())
    .bind(claim.message_id.as_str())
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?;

    let Some((generation, status, lease)) = row else {
        tx.rollback().await.ok();
        return Err(AppError::not_found("聚合任务不存在"));
    };
    if generation != claim.generation
        || status != AggregateJobStatus::Processing.as_str()
        || lease.as_deref() != Some(claim.lease_token.as_str())
    {
        tx.rollback().await.ok();
        return Err(AppError::conflict("聚合任务租约失效"));
    }

    if attempts > MAX_ATTEMPTS_PER_GENERATION {
        jobs::fail_permanently(&mut tx, claim, "超过本代最大重试次数")
            .await
            .map_err(AppError::from)?;
        tx.commit().await?;
        return Ok((AggregationOutcome::Skipped, None));
    }

    let account = accounts::get(&mut tx, &claim.account_id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", claim.account_id)))?;

    let Some(bundle) = messages::get(&mut tx, &claim.account_id, &claim.message_id)
        .await
        .map_err(AppError::from)?
    else {
        jobs::fail_permanently(&mut tx, claim, "邮件不存在")
            .await
            .map_err(AppError::from)?;
        tx.commit().await?;
        return Ok((AggregationOutcome::Skipped, None));
    };

    if bundle.message.sync_status == MessageSyncStatus::DeletedRemote {
        jobs::complete(
            &mut tx,
            &claim.account_id,
            &claim.message_id,
            &claim.lease_token,
            claim.generation,
            AggregationOutcome::Skipped,
        )
        .await
        .map_err(AppError::from)?;
        tx.commit().await?;
        return Ok((AggregationOutcome::Skipped, None));
    }

    let parent = resolve_parent_hint(&mut tx, &claim.account_id, &bundle).await?;

    let self_set = resolve_self_addresses(&account);
    let plan = match decide_aggregation(&AggregationContext {
        message: bundle.clone(),
        self_set,
        parent,
    }) {
        Ok(p) => p,
        Err(e) => {
            jobs::fail_permanently(&mut tx, claim, &e.to_string())
                .await
                .map_err(AppError::from)?;
            tx.commit().await?;
            return Ok((AggregationOutcome::Skipped, None));
        }
    };

    let apply_result = apply_plan(&mut tx, &claim.account_id, &bundle, &plan).await;
    match apply_result {
        Ok(cs) => {
            jobs::complete(
                &mut tx,
                &claim.account_id,
                &claim.message_id,
                &claim.lease_token,
                claim.generation,
                plan.outcome,
            )
            .await
            .map_err(AppError::from)?;
            tx.commit().await?;
            Ok((plan.outcome, Some(cs)))
        }
        Err(e) => {
            let retryable = matches!(
                e.code,
                crate::application::error::AppErrorCode::Network
                    | crate::application::error::AppErrorCode::Sync
                    | crate::application::error::AppErrorCode::Internal
            ) && e.retryable;
            if retryable && attempts < MAX_ATTEMPTS_PER_GENERATION {
                jobs::fail_for_retry(
                    &mut tx,
                    &claim.account_id,
                    &claim.message_id,
                    &claim.lease_token,
                    claim.generation,
                    &e.message,
                    now_ms() + backoff_ms(attempts),
                )
                .await
                .map_err(AppError::from)?;
                tx.commit().await?;
                Err(e)
            } else {
                jobs::fail_permanently(&mut tx, claim, &e.message)
                    .await
                    .map_err(AppError::from)?;
                tx.commit().await?;
                Err(e)
            }
        }
    }
}

/// in_reply_to → internet_message_id → session resolve_root（含 kind / 成员，供决策兼容性）
async fn resolve_parent_hint(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    bundle: &MessageWithRecipients,
) -> Result<Option<ParentSessionHint>, AppError> {
    let Some(irt) = bundle
        .message
        .in_reply_to
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    // In-Reply-To 可能含多个 id；取第一个
    let first = irt
        .split_whitespace()
        .next()
        .unwrap_or(irt)
        .trim()
        .to_string();
    let Some(parent_msg) = messages::find_by_internet_message_id(conn, account_id, &first)
        .await
        .map_err(AppError::from)?
    else {
        return Ok(None);
    };
    let Some(root) = sessions::resolve_root(conn, account_id, &parent_msg.message.session_id)
        .await
        .map_err(AppError::from)?
    else {
        return Ok(None);
    };
    let group_member_addresses = if root.kind == SessionKind::Group {
        if let Some(ref gid) = root.group_id {
            groups::list_participants(conn, account_id, gid)
                .await
                .map_err(AppError::from)?
                .into_iter()
                .map(|p| p.normalized_address)
                .collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    Ok(Some(ParentSessionHint {
        id: root.id,
        kind: root.kind,
        normalized_address: root.normalized_address,
        group_id: root.group_id,
        group_member_addresses,
    }))
}

async fn apply_plan(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    bundle: &MessageWithRecipients,
    plan: &AggregationPlan,
) -> Result<ChangeSet, AppError> {
    let now = now_ms();
    let message_id = &bundle.message.id;
    let old_session_id = bundle.message.session_id.clone();
    let mut touched: HashSet<String> = HashSet::new();
    touched.insert(old_session_id.as_str().to_string());

    let Some(target) = &plan.session_target else {
        // Skipped：保持原 session，仍重算投影
        sessions::recompute_projection(conn, account_id, &old_session_id)
            .await
            .map_err(AppError::from)?;
        return Ok(ChangeSet {
            account_id: account_id.clone(),
            session_ids: vec![old_session_id],
            message_ids: vec![message_id.clone()],
            new_incoming_ids: Vec::new(),
            origin: ChangeOrigin::Rebuild,
        });
    };

    let session_id = match target {
        SessionTarget::Existing(sid) => {
            let root = sessions::resolve_root(conn, account_id, sid)
                .await
                .map_err(AppError::from)?
                .ok_or_else(|| AppError::not_found(format!("会话不存在: {sid}")))?;
            root.id
        }
        SessionTarget::Private { address } => {
            let observed_name = plan
                .observed_participants
                .iter()
                .find(|(a, _)| a == address)
                .map(|(_, n)| n.as_str());
            ensure_private_session(conn, account_id, address, observed_name, now).await?
        }
        SessionTarget::Group { target } => {
            let group_id = match target {
                GroupTarget::Existing(id) => id.clone(),
                GroupTarget::Create { members } => {
                    // 同成员集合复用已有自动群，避免「lida+zhang」每次新建/或被 In-Reply-To 锁死在私聊
                    if let Some(existing) =
                        groups::find_auto_group_by_exact_members(conn, account_id, members)
                            .await
                            .map_err(AppError::from)?
                    {
                        existing
                    } else {
                        create_auto_group(
                            conn,
                            account_id,
                            members,
                            &plan.observed_participants,
                            now,
                        )
                        .await?
                    }
                }
            };
            // 观测成员：人表写 observed；成员行存短名
            let mut parts = Vec::new();
            for (addr, name) in &plan.observed_participants {
                let contact = ensure_observed_contact(
                    conn,
                    account_id,
                    addr,
                    Some(name.as_str()).filter(|s| !s.is_empty()),
                    now,
                )
                .await?;
                let label = if contact.source.is_address_book() {
                    contact.display_name.as_str()
                } else {
                    name.as_str()
                };
                parts.push(GroupParticipant {
                    account_id: account_id.clone(),
                    group_id: group_id.clone(),
                    normalized_address: addr.clone(),
                    display_name: crate::domain::services::person_label(label, addr.as_str()),
                    is_observed: true,
                    is_send_member: false,
                    first_seen_at: now,
                    last_seen_at: now,
                });
            }
            if !parts.is_empty() {
                groups::upsert_participants(conn, &parts)
                    .await
                    .map_err(AppError::from)?;
            }
            refresh_auto_group_name(conn, account_id, &group_id, now).await?;
            ensure_group_session(conn, account_id, &group_id, now).await?
        }
    };

    if session_id != old_session_id {
        messages::set_session_id(conn, account_id, message_id, &session_id)
            .await
            .map_err(AppError::from)?;
    }
    touched.insert(session_id.as_str().to_string());

    let mut session_ids = Vec::new();
    for sid in &touched {
        let id = SessionId::new(sid.clone());
        sessions::recompute_projection(conn, account_id, &id)
            .await
            .map_err(AppError::from)?;
        session_ids.push(id);
    }

    Ok(ChangeSet {
        account_id: account_id.clone(),
        session_ids,
        message_ids: vec![message_id.clone()],
        new_incoming_ids: Vec::new(),
        origin: ChangeOrigin::Rebuild,
    })
}

fn usable_contact_display_name(name: &str, address: &EmailAddress) -> bool {
    crate::domain::services::usable_person_label(name, address.as_str())
}

fn auto_group_title_from_observed(
    members: &[EmailAddress],
    observed: &[(EmailAddress, String)],
) -> String {
    crate::domain::services::auto_group_title(members.iter().map(|addr| {
        let name = observed
            .iter()
            .find(|(a, _)| a == addr)
            .map(|(_, n)| n.as_str())
            .unwrap_or("");
        (addr.as_str(), name)
    }))
}

/// 确保人表有一行：已有真联系人则复用；否则建/回填 observed（不进通讯录页）
async fn ensure_observed_contact(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    peer: &EmailAddress,
    observed_name: Option<&str>,
    now: TimestampMs,
) -> Result<Contact, AppError> {
    let better_name = observed_name
        .map(str::trim)
        .filter(|n| usable_contact_display_name(n, peer));

    if let Some(mut c) = contacts::get_by_address(conn, account_id, peer)
        .await
        .map_err(AppError::from)?
    {
        if c.source == ContactSource::Observed {
            if let Some(name) = better_name {
                if !usable_contact_display_name(&c.display_name, peer) {
                    c.display_name = name.to_owned();
                    c.updated_at = now;
                    contacts::upsert(conn, &c)
                        .await
                        .map_err(AppError::from)?;
                }
            }
        }
        return Ok(c);
    }

    let c = Contact {
        id: ContactId::new(Uuid::new_v4().to_string()),
        account_id: account_id.clone(),
        graph_id: None,
        display_name: better_name.unwrap_or(peer.as_str()).to_owned(),
        normalized_address: peer.clone(),
        source: ContactSource::Observed,
        created_at: now,
        updated_at: now,
    };
    contacts::upsert(conn, &c)
        .await
        .map_err(AppError::from)?;
    Ok(c)
}

async fn ensure_private_session(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    peer: &EmailAddress,
    observed_name: Option<&str>,
    now: TimestampMs,
) -> Result<SessionId, AppError> {
    if let Some(existing) = sessions::get_private_by_address(conn, account_id, peer)
        .await
        .map_err(AppError::from)?
    {
        let root = if existing.merged_into_id.is_some() {
            sessions::resolve_root(conn, account_id, &existing.id)
                .await
                .map_err(AppError::from)?
                .unwrap_or(existing)
        } else {
            existing
        };
        let _ = ensure_observed_contact(conn, account_id, peer, observed_name, now).await?;
        return Ok(root.id);
    }

    let contact = ensure_observed_contact(conn, account_id, peer, observed_name, now).await?;

    let session = sessions::new_private(
        account_id.clone(),
        peer.clone(),
        Some(contact.id),
        now,
    );
    sessions::upsert(conn, &session)
        .await
        .map_err(AppError::from)?;
    Ok(session.id)
}

async fn ensure_group_session(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
    now: TimestampMs,
) -> Result<SessionId, AppError> {
    if let Some(existing) = sessions::get_group_by_group_id(conn, account_id, group_id)
        .await
        .map_err(AppError::from)?
    {
        if existing.merged_into_id.is_some() {
            let root = sessions::resolve_root(conn, account_id, &existing.id)
                .await
                .map_err(AppError::from)?
                .unwrap_or(existing);
            return Ok(root.id);
        }
        return Ok(existing.id);
    }
    let session = sessions::new_group(account_id.clone(), group_id.clone(), now);
    sessions::upsert(conn, &session)
        .await
        .map_err(AppError::from)?;
    Ok(session.id)
}

async fn create_auto_group(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    members: &[EmailAddress],
    observed: &[(EmailAddress, String)],
    now: TimestampMs,
) -> Result<GroupId, AppError> {
    let name = auto_group_title_from_observed(members, observed);
    let group = Group {
        id: GroupId::new(Uuid::new_v4().to_string()),
        account_id: account_id.clone(),
        name,
        name_is_manual: false,
        source: GroupSource::Auto,
        send_members_is_manual: false,
        created_at: now,
        updated_at: now,
    };
    groups::insert(conn, &group)
        .await
        .map_err(AppError::from)?;

    let mut parts = Vec::new();
    for addr in members {
        let raw = observed
            .iter()
            .find(|(a, _)| a == addr)
            .map(|(_, n)| n.as_str())
            .unwrap_or("");
        let contact = ensure_observed_contact(conn, account_id, addr, Some(raw).filter(|s| !s.is_empty()), now)
            .await?;
        let display = crate::domain::services::person_label(
            if contact.source.is_address_book() {
                contact.display_name.as_str()
            } else {
                raw
            },
            addr.as_str(),
        );
        parts.push(GroupParticipant {
            account_id: account_id.clone(),
            group_id: group.id.clone(),
            normalized_address: addr.clone(),
            display_name: display,
            is_observed: true,
            is_send_member: true,
            first_seen_at: now,
            last_seen_at: now,
        });
    }
    groups::upsert_participants(conn, &parts)
        .await
        .map_err(AppError::from)?;
    Ok(group.id)
}

/// 非手动群名：随观测昵称刷新（优先名称）
async fn refresh_auto_group_name(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
    now: TimestampMs,
) -> Result<(), AppError> {
    let Some(mut group) = groups::get(conn, account_id, group_id)
        .await
        .map_err(AppError::from)?
    else {
        return Ok(());
    };
    if group.name_is_manual {
        return Ok(());
    }
    let title = groups::resolve_auto_title(conn, account_id, group_id)
        .await
        .map_err(AppError::from)?;
    if title == group.name {
        return Ok(());
    }
    group.name = title;
    group.updated_at = now;
    groups::update(conn, &group)
        .await
        .map_err(AppError::from)?;
    Ok(())
}

/// 将账户内邮件重新入队。
pub async fn requeue_account_messages(
    db: &SqlitePool,
    account_id: &AccountId,
    limit: u32,
) -> Result<u32, AppError> {
    let mut tx = db.begin().await?;
    let ids = jobs::list_message_ids_for_reaggregate(&mut tx, account_id, limit)
        .await
        .map_err(AppError::from)?;
    let now = now_ms();
    for id in &ids {
        jobs::enqueue_or_reset(&mut tx, account_id, id, now)
            .await
            .map_err(AppError::from)?;
    }
    tx.commit().await?;
    Ok(ids.len() as u32)
}

// 供同步路径创建临时私聊 Session
pub async fn ensure_provisional_private(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    address: &EmailAddress,
    now: TimestampMs,
) -> Result<SessionId, AppError> {
    ensure_private_session(conn, account_id, address, None, now).await
}

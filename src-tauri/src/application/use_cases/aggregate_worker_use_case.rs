//! 聚合后台用例：领取 jobs → decide_aggregation → 写投影 → complete。

use crate::application::dto::{AggregateClaim, ChangeOrigin, ChangeSet};
use crate::application::error::AppError;
use crate::domain::entities::{
    Contact, Conversation, ConversationMessageLink, Group, GroupParticipant, GroupThread,
    MessageWithRecipients,
};
use crate::domain::services::{
    decide_aggregation, resolve_self_addresses, AggregationContext, AggregationPlan, GroupTarget,
};
use crate::domain::value_objects::{
    AccountId, AggregateJobStatus, AggregationOutcome, ContactId, ConversationId, ConversationKind,
    EmailAddress, GroupId, GroupSource, MessageSyncStatus, TimestampMs,
};
use crate::infrastructure::repository::{
    accounts, contacts, conversations, groups, jobs, messages,
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
                                acc.conversation_ids.extend(cs.conversation_ids);
                                acc.new_incoming_ids.extend(cs.new_incoming_ids);
                            }
                            None => changeset_acc = Some(cs),
                        }
                    }
                }
                Err(e) => {
                    // aggregate_message 内部已写 retry/fail；此处仅继续
                    let _ = e;
                }
            }
        }
    }

    if let (Some(dispatcher), Some(mut cs)) = (changes, changeset_acc) {
        let mut seen = HashSet::new();
        cs.conversation_ids.retain(|c| seen.insert(c.clone()));
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

    // 校验仍持有该代次租约
    let row = sqlx::query_as::<_, (i64, String, Option<String>)>(
        "SELECT generation, status, lease_token FROM conversation_aggregate_jobs
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

    let thread_group = if let Some(tid) = &bundle.message.thread_id {
        if let Some(link) = groups::get_thread_link(&mut tx, &claim.account_id, tid)
            .await
            .map_err(AppError::from)?
        {
            groups::resolve_root(&mut tx, &claim.account_id, &link.group_id)
                .await
                .map_err(AppError::from)?
                .map(|g| g.id)
        } else {
            None
        }
    } else {
        None
    };

    let self_set = resolve_self_addresses(&account);
    let plan = match decide_aggregation(&AggregationContext {
        message: bundle.clone(),
        self_set,
        thread_group,
        show_group_messages_in_contact_conversation: account
            .show_group_messages_in_contact_conversation,
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
            // 可恢复 → retry；否则永久失败
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

async fn apply_plan(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    bundle: &MessageWithRecipients,
    plan: &AggregationPlan,
) -> Result<ChangeSet, AppError> {
    let now = now_ms();
    let message_id = &bundle.message.id;
    let mut touched: HashSet<String> = HashSet::new();
    let mut desired: HashSet<String> = HashSet::new();

    // --- 联系人会话 ---
    for addr in &plan.contact_addresses {
        let observed_name = plan
            .observed_participants
            .iter()
            .find(|(a, _)| a == addr)
            .map(|(_, n)| n.as_str());
        let cid =
            ensure_contact_conversation(conn, account_id, addr, observed_name, now).await?;
        desired.insert(cid.as_str().to_string());
        conversations::attach_messages(
            conn,
            &[ConversationMessageLink {
                account_id: account_id.clone(),
                conversation_id: cid.clone(),
                message_id: message_id.clone(),
                attached_at: now,
            }],
        )
        .await
        .map_err(AppError::from)?;
        touched.insert(cid.into_inner());
    }

    // --- 群组会话 ---
    if let Some(target) = &plan.group_target {
        let group_id = match target {
            GroupTarget::Existing(id) => {
                groups::resolve_root(conn, account_id, id)
                    .await
                    .map_err(AppError::from)?
                    .map(|g| g.id)
                    .ok_or_else(|| AppError::not_found(format!("群组不存在: {id}")))?
            }
            GroupTarget::Create { members } => {
                create_auto_group(conn, account_id, members, &plan.observed_participants, now)
                    .await?
            }
        };

        if let Some(tid) = &bundle.message.thread_id {
            groups::upsert_thread_link(
                conn,
                &GroupThread {
                    account_id: account_id.clone(),
                    group_id: group_id.clone(),
                    thread_id: tid.clone(),
                    linked_at: now,
                },
            )
            .await
            .map_err(AppError::from)?;
        }

        // 观测成员
        let mut parts = Vec::new();
        for (addr, name) in &plan.observed_participants {
            parts.push(GroupParticipant {
                account_id: account_id.clone(),
                group_id: group_id.clone(),
                normalized_address: addr.clone(),
                display_name: name.clone(),
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

        let gconv = ensure_group_conversation(conn, account_id, &group_id, now).await?;
        desired.insert(gconv.as_str().to_string());
        conversations::attach_messages(
            conn,
            &[ConversationMessageLink {
                account_id: account_id.clone(),
                conversation_id: gconv.clone(),
                message_id: message_id.clone(),
                attached_at: now,
            }],
        )
        .await
        .map_err(AppError::from)?;
        touched.insert(gconv.into_inner());
    }

    // 脱离不在期望集合中的会话附着
    let current = conversations::find_conversation_ids_by_message(conn, account_id, message_id)
        .await
        .map_err(AppError::from)?;
    for cid in current {
        if !desired.contains(cid.as_str()) {
            conversations::detach_messages(conn, account_id, &cid, &[message_id.clone()])
                .await
                .map_err(AppError::from)?;
            touched.insert(cid.into_inner());
        }
    }

    let mut conversation_ids = Vec::new();
    for cid in &touched {
        let id = ConversationId::new(cid.clone());
        conversations::recompute_projection(conn, account_id, &id)
            .await
            .map_err(AppError::from)?;
        conversation_ids.push(id);
    }

    Ok(ChangeSet {
        account_id: account_id.clone(),
        conversation_ids,
        message_ids: vec![message_id.clone()],
        new_incoming_ids: Vec::new(),
        origin: ChangeOrigin::Rebuild,
    })
}

/// 邮件里的展示名是否比「邮箱占位」更适合作为联系人昵称。
fn usable_contact_display_name(name: &str, address: &EmailAddress) -> bool {
    let n = name.trim();
    if n.is_empty() {
        return false;
    }
    !n.eq_ignore_ascii_case(address.as_str())
}

async fn ensure_contact_conversation(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    peer: &EmailAddress,
    observed_name: Option<&str>,
    now: TimestampMs,
) -> Result<ConversationId, AppError> {
    let better_name = observed_name
        .map(str::trim)
        .filter(|n| usable_contact_display_name(n, peer));

    if let Some(existing) = conversations::get_contact_conversation(conn, account_id, peer)
        .await
        .map_err(AppError::from)?
    {
        if let (Some(name), Some(cid)) = (better_name, existing.contact_id.as_ref()) {
            if let Some(mut contact) = contacts::get(conn, account_id, cid)
                .await
                .map_err(AppError::from)?
            {
                if !usable_contact_display_name(&contact.display_name, peer) {
                    contact.display_name = name.to_owned();
                    contact.updated_at = now;
                    contacts::upsert(conn, &contact)
                        .await
                        .map_err(AppError::from)?;
                }
            }
        }
        return Ok(existing.id);
    }
    let contact = if let Some(mut c) = contacts::get_by_address(conn, account_id, peer)
        .await
        .map_err(AppError::from)?
    {
        if let Some(name) = better_name {
            if !usable_contact_display_name(&c.display_name, peer) {
                c.display_name = name.to_owned();
                c.updated_at = now;
                contacts::upsert(conn, &c)
                    .await
                    .map_err(AppError::from)?;
            }
        }
        c
    } else {
        let c = Contact {
            id: ContactId::new(Uuid::new_v4().to_string()),
            account_id: account_id.clone(),
            graph_id: None,
            display_name: better_name
                .unwrap_or(peer.as_str())
                .to_owned(),
            normalized_address: peer.clone(),
            created_at: now,
            updated_at: now,
        };
        contacts::upsert(conn, &c).await.map_err(AppError::from)?;
        c
    };
    let cid = ConversationId::new(format!("conv-{}", Uuid::new_v4()));
    conversations::upsert(
        conn,
        &Conversation {
            id: cid.clone(),
            account_id: account_id.clone(),
            kind: ConversationKind::Contact,
            normalized_address: Some(peer.clone()),
            contact_id: Some(contact.id),
            group_id: None,
            last_message_at: None,
            last_message_id: None,
            unread_count: 0,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .map_err(AppError::from)?;
    Ok(cid)
}

async fn ensure_group_conversation(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
    now: TimestampMs,
) -> Result<ConversationId, AppError> {
    if let Some(existing) = conversations::get_group_conversation(conn, account_id, group_id)
        .await
        .map_err(AppError::from)?
    {
        return Ok(existing.id);
    }
    let cid = ConversationId::new(format!("conv-{}", Uuid::new_v4()));
    conversations::upsert(
        conn,
        &Conversation {
            id: cid.clone(),
            account_id: account_id.clone(),
            kind: ConversationKind::Group,
            normalized_address: None,
            contact_id: None,
            group_id: Some(group_id.clone()),
            last_message_at: None,
            last_message_id: None,
            unread_count: 0,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .map_err(AppError::from)?;
    Ok(cid)
}

async fn create_auto_group(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    members: &[EmailAddress],
    observed: &[(EmailAddress, String)],
    now: TimestampMs,
) -> Result<GroupId, AppError> {
    let name = members
        .iter()
        .take(3)
        .map(|a| a.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let group = Group {
        id: GroupId::new(Uuid::new_v4().to_string()),
        account_id: account_id.clone(),
        name: if name.is_empty() {
            "群组".into()
        } else {
            name
        },
        name_is_manual: false,
        source: GroupSource::Auto,
        send_members_is_manual: false,
        merged_into_id: None,
        created_at: now,
        updated_at: now,
    };
    groups::insert(conn, &group)
        .await
        .map_err(AppError::from)?;

    let mut parts = Vec::new();
    for addr in members {
        let display = observed
            .iter()
            .find(|(a, _)| a == addr)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| addr.as_str().to_string());
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

/// 将账户内邮件重新入队（显示配置变更后）。
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

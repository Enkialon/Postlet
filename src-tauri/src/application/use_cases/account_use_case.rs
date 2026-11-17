//! 账户用例
use crate::infrastructure::config::load_azure_auth_config;
use crate::infrastructure::graph::auth as graph_auth;
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::accounts;
use crate::runtime::{ChangeDispatcher, MailSyncGate};
use crate::AppState;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::application::dto::{
    ChangeOrigin, ChangeSet, UpdateAccountAliasesCommand, UpdateAccountDisplaySettingsCommand,
};
use crate::application::error::AppError;
use crate::application::use_cases::aggregate_worker_use_case;
use crate::application::use_cases::compose_use_case;
use crate::domain::entities::Account;
use crate::domain::value_objects::{AccountId, EmailAddress, SyncStatus, TimestampMs};

/// 列出当前已登录的 Outlook 账户，供启动恢复和账户切换使用。
///
/// 已退出（仅清凭证）的账户行仍在库中，但不出现在此列表；不发起登录或远端同步。
pub async fn list_accounts(db: &SqlitePool) -> Result<Vec<Account>, AppError> {
    let mut conn = db.acquire().await?;
    accounts::list(&mut conn).await.map_err(Into::into)
}

/// 按本地账户 ID 获取账户资料；不存在时返回未找到错误。
pub async fn get_account(db: &SqlitePool, account_id: &AccountId) -> Result<Account, AppError> {
    let mut conn = db.acquire().await?;
    accounts::get(&mut conn, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))
}

/// 登录 Outlook：OAuth（Auth Code + PKCE）→ 本地账户 upsert → 凭证落盘。
pub async fn sign_in(db: &SqlitePool, paths: &AppPaths) -> Result<Account, AppError> {
    let config = load_azure_auth_config(&paths.config_dir).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("未找到 Azure 配置") || msg.contains("未配置 Azure") {
            AppError::validation(msg)
        } else {
            AppError::from(e)
        }
    })?;

    let (remote, token_cache) =
        graph_auth::sign_in(&paths.credentials_dir, &config).await?;

    let now = now_ms();
    let mut tx = db.begin().await?;

    let existing = if let Some(home) = remote.home_account_id.as_deref() {
        accounts::get_by_home_account_id(&mut tx, home).await?
    } else {
        None
    };
    let existing = match existing {
        Some(acc) => Some(acc),
        None => accounts::get_by_email(&mut tx, &remote.email).await?,
    };

    let account = if let Some(existing) = existing {
        Account {
            id: existing.id,
            email: remote.email.clone(),
            display_name: remote.display_name.clone(),
            // 别名仅手动维护，登录刷新不得冲掉
            aliases: existing.aliases,
            tenant_id: remote.tenant_id.clone().or(existing.tenant_id),
            home_account_id: remote
                .home_account_id
                .clone()
                .or(existing.home_account_id),
            contact_delta_link: existing.contact_delta_link,
            mail_sync_triggered_at: existing.mail_sync_triggered_at,
            contact_synced_at: existing.contact_synced_at,
            mail_sync_status: existing.mail_sync_status,
            mail_sync_error: existing.mail_sync_error,
            include_in_all: existing.include_in_all,
            created_at: existing.created_at,
            updated_at: now,
        }
    } else {
        Account {
            id: AccountId::new(Uuid::new_v4().to_string()),
            email: remote.email.clone(),
            display_name: remote.display_name.clone(),
            aliases: Vec::new(),
            tenant_id: remote.tenant_id.clone(),
            home_account_id: remote.home_account_id.clone(),
            contact_delta_link: None,
            mail_sync_triggered_at: None,
            contact_synced_at: None,
            mail_sync_status: SyncStatus::Idle,
            mail_sync_error: None,
            include_in_all: true,
            created_at: now,
            updated_at: now,
        }
    };

    accounts::upsert_identity(&mut tx, &account).await?;
    crate::infrastructure::repository::mail_transports::ensure_default(
        &mut tx,
        &account.id,
        now,
    )
    .await
    .map_err(AppError::from)?;
    let saved = accounts::get(&mut tx, &account.id)
        .await?
        .ok_or_else(|| AppError::internal("登录后读取账户失败"))?;
    tx.commit().await?;

    graph_auth::save_token_cache(&paths.credentials_dir, &saved.id, &token_cache).await?;
    Ok(saved)
}

/// 退出指定账户：删除本地凭证并标记未登录；保留账户行与本地邮件。
///
/// 再登录同一微软账户会复用原 `AccountId` 与本地数据，再拉取离线期间邮件。
/// 列表在 `account_id` 为空时只 `IN` 已登录且 `include_in_all` 的账号。
pub async fn sign_out(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
) -> Result<(), AppError> {
    let mut conn = db.acquire().await?;
    let _ = accounts::get(&mut conn, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    graph_auth::sign_out(&paths.credentials_dir, account_id).await?;
    let _credential_lock = crate::infrastructure::mail::credentials::lock(&paths.credentials_dir)?;
    if let Some(reference) = crate::infrastructure::repository::mail_transports::get(&mut conn, account_id)
        .await?.and_then(|transport| transport.credential_ref) {
        crate::infrastructure::mail::credentials::delete(&paths.credentials_dir, &reference).await?;
    }
    accounts::set_signed_in(&mut conn, account_id, false)
        .await
        .map_err(AppError::from)?;
    Ok(())
}

/// 清理指定账号的本地聊天/同步数据；保留账号行、登录态与传输配置。
///
/// 与 `sign_out` 相反侧重：不删凭据、不改 `is_signed_in`；删会话/邮件/联系人/群与同步游标。
/// 邮件同步进行中时拒绝，避免与写入竞态。
pub async fn clear_local_data(
    state: &AppState,
    account_id: &AccountId,
) -> Result<(), AppError> {
    let gate: &MailSyncGate = &state.mail_sync_gate;
    let _permit = gate.try_acquire(account_id.as_str()).ok_or_else(|| {
        AppError::conflict("邮件同步进行中，请稍后再清理本地数据")
    })?;
    clear_local_data_inner(state, account_id).await
}

async fn clear_local_data_inner(
    state: &AppState,
    account_id: &AccountId,
) -> Result<(), AppError> {
    {
        let mut conn = state.db.acquire().await?;
        let _ = accounts::get(&mut conn, account_id)
            .await?
            .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    }

    let mut tx = state.db.begin().await?;
    accounts::clear_local_chat_data(&mut tx, account_id)
        .await
        .map_err(AppError::from)?;
    tx.commit().await?;

    compose_use_case::clear_drafts_for_account(state, account_id)?;

    let mut changeset = ChangeSet::empty(account_id.clone(), ChangeOrigin::UserAction);
    changeset.account_cleared = true;
    state.changes.dispatch(changeset);
    Ok(())
}

/// 更新账户显示配置（是否纳入「全部账号」列表）。
pub async fn update_display_settings(
    db: &SqlitePool,
    cmd: UpdateAccountDisplaySettingsCommand,
) -> Result<Account, AppError> {
    let mut tx = db.begin().await?;
    accounts::update_display_settings(&mut tx, &cmd.account_id, cmd.include_in_all)
        .await
        .map_err(AppError::from)?;
    let account = accounts::get(&mut tx, &cmd.account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;
    tx.commit().await?;
    Ok(account)
}

/// 手动更新账户别名（self 额外身份；整表替换）。
///
/// 集合变化时：把「对端已变 self」的私聊消息迁到正确会话（读路径动态判定 is_outbound）。
pub async fn update_aliases(
    db: &SqlitePool,
    changes: &ChangeDispatcher,
    cmd: UpdateAccountAliasesCommand,
) -> Result<Account, AppError> {
    let mut tx = db.begin().await?;
    let account = accounts::get(&mut tx, &cmd.account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;

    let aliases = normalize_aliases(&account.email, &cmd.aliases)?;
    let aliases_changed = aliases != account.aliases;
    ensure_aliases_not_claimed(&mut tx, &account.id, &aliases).await?;

    accounts::update_aliases(&mut tx, &cmd.account_id, &aliases)
        .await
        .map_err(AppError::from)?;
    let updated = accounts::get(&mut tx, &cmd.account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;
    tx.commit().await?;

    if aliases_changed {
        let _ = aggregate_worker_use_case::relocate_self_keyed_private_sessions(
            db,
            &updated.id,
            Some(changes),
        )
        .await?;
    }
    Ok(updated)
}

fn normalize_aliases(
    primary: &EmailAddress,
    raw: &[String],
) -> Result<Vec<EmailAddress>, AppError> {
    let mut out = Vec::new();
    for item in raw {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !trimmed.contains('@') {
            return Err(AppError::validation(format!("邮箱地址无效: {trimmed}")));
        }
        let Some(addr) = EmailAddress::parse(trimmed) else {
            continue;
        };
        if &addr == primary {
            return Err(AppError::validation(
                "别名不能与主邮箱相同".to_string(),
            ));
        }
        if out.iter().any(|a| a == &addr) {
            continue;
        }
        out.push(addr);
    }
    Ok(out)
}

async fn ensure_aliases_not_claimed(
    conn: &mut sqlx::SqliteConnection,
    account_id: &AccountId,
    aliases: &[EmailAddress],
) -> Result<(), AppError> {
    if aliases.is_empty() {
        return Ok(());
    }
    let others = accounts::list_all(conn).await.map_err(AppError::from)?;
    for other in others {
        if &other.id == account_id {
            continue;
        }
        for alias in aliases {
            if alias == &other.email {
                return Err(AppError::validation(format!(
                    "别名已被其他账号主邮箱占用: {alias}"
                )));
            }
            if other.aliases.iter().any(|a| a == alias) {
                return Err(AppError::validation(format!(
                    "别名已被其他账号使用: {alias}"
                )));
            }
        }
    }
    Ok(())
}

fn now_ms() -> TimestampMs {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

//! 账户用例
use crate::infrastructure::config::load_azure_auth_config;
use crate::infrastructure::graph::auth as graph_auth;
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::accounts;
use crate::infrastructure::seed;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::application::dto::UpdateAccountDisplaySettingsCommand;
use crate::application::error::AppError;
use crate::domain::entities::Account;
use crate::domain::value_objects::{AccountId, SyncStatus, TimestampMs};

/// 列出本地保存的 Outlook 账户，供启动恢复和账户切换使用。
///
/// 只查询本地资料，不发起登录或远端同步。
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
            aliases: remote.aliases.clone(),
            tenant_id: remote.tenant_id.clone().or(existing.tenant_id),
            home_account_id: remote
                .home_account_id
                .clone()
                .or(existing.home_account_id),
            contact_delta_link: existing.contact_delta_link,
            mail_synced_at: existing.mail_synced_at,
            contact_synced_at: existing.contact_synced_at,
            mail_sync_status: existing.mail_sync_status,
            mail_sync_error: existing.mail_sync_error,
            show_group_messages_in_contact_conversation: existing
                .show_group_messages_in_contact_conversation,
            created_at: existing.created_at,
            updated_at: now,
        }
    } else {
        Account {
            id: AccountId::new(Uuid::new_v4().to_string()),
            email: remote.email.clone(),
            display_name: remote.display_name.clone(),
            aliases: remote.aliases.clone(),
            tenant_id: remote.tenant_id.clone(),
            home_account_id: remote.home_account_id.clone(),
            contact_delta_link: None,
            mail_synced_at: None,
            contact_synced_at: None,
            mail_sync_status: SyncStatus::Idle,
            mail_sync_error: None,
            show_group_messages_in_contact_conversation: false,
            created_at: now,
            updated_at: now,
        }
    };

    accounts::upsert_identity(&mut tx, &account).await?;
    let saved = accounts::get(&mut tx, &account.id)
        .await?
        .ok_or_else(|| AppError::internal("登录后读取账户失败"))?;
    tx.commit().await?;

    graph_auth::save_token_cache(&paths.credentials_dir, &saved.id, &token_cache).await?;
    Ok(saved)
}

/// 退出指定账户：删除本地凭证；保留账户行与本地邮件。
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
    Ok(())
}

/// 更新群组邮件是否同时显示在联系人会话中的账户配置，并返回账户资料。
pub async fn update_display_settings(
    db: &SqlitePool,
    cmd: UpdateAccountDisplaySettingsCommand,
) -> Result<Account, AppError> {
    let mut tx = db.begin().await?;
    accounts::update_display_settings(
        &mut tx,
        &cmd.account_id,
        cmd.show_group_messages_in_contact_conversation,
    )
    .await
    .map_err(AppError::from)?;
    let account = accounts::get(&mut tx, &cmd.account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;
    tx.commit().await?;
    Ok(account)
}

/// 清空并装载本地演示工作区（不接 Graph）。
pub async fn load_demo_workspace(db: &SqlitePool) -> Result<Account, AppError> {
    let mut tx = db.begin().await?;
    let account = seed::seed_demo_workspace(&mut tx).await?;
    tx.commit().await?;
    Ok(account)
}

fn now_ms() -> TimestampMs {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

//! 账户用例
use crate::infrastructure::repository::accounts;
use crate::infrastructure::seed;
use sqlx::SqlitePool;

use crate::application::dto::UpdateAccountDisplaySettingsCommand;
use crate::application::error::AppError;
use crate::domain::entities::Account;
use crate::domain::value_objects::AccountId;

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

/// 登录 Outlook 并建立或刷新本地账户资料。
///
/// 当前不接 OAuth；请使用 `load_demo_workspace` 装载本地演示数据。
pub async fn sign_in(db: &SqlitePool) -> Result<Account, AppError> {
    let _ = db;
    Err(AppError::not_implemented(
        "account_use_case::sign_in（未接 OAuth；请用 load_demo_workspace）",
    ))
}

/// 退出指定 Outlook 账户的登录状态。
///
/// 当前为骨架，返回未实现错误；本地邮件是否保留仍需另行确定。
pub async fn sign_out(db: &SqlitePool, account_id: &AccountId) -> Result<(), AppError> {
    let _ = (db, account_id);
    Err(AppError::not_implemented("account_use_case::sign_out"))
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

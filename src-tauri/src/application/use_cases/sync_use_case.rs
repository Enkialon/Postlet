//! 同步用例
use crate::infrastructure::repository::{accounts, folder_states};
use sqlx::SqlitePool;

use crate::application::dto::{
    ContactSyncProgress, MailSyncProgress, TriggerContactSyncCommand, TriggerMailSyncCommand,
};
use crate::application::error::AppError;
use crate::domain::value_objects::AccountId;

/// 请求同步指定账户的 Outlook 邮件，未指定文件夹时使用当前跟踪范围。
///
/// 各页邮件变更、聚合任务和同步游标须原子落库；触发成功不等于同步完成。
/// 当前为骨架，返回未实现错误。
pub async fn trigger_mail_sync(
    db: &SqlitePool,
    cmd: TriggerMailSyncCommand,
) -> Result<(), AppError> {
    let _ = (db, cmd);
    Err(AppError::not_implemented(
        "sync_use_case::trigger_mail_sync",
    ))
}

/// 请求同步指定账户的 Outlook 通讯录，并更新本地联系人缓存。
///
/// 触发成功不等于同步完成；当前为骨架，返回未实现错误。
pub async fn trigger_contact_sync(
    db: &SqlitePool,
    cmd: TriggerContactSyncCommand,
) -> Result<(), AppError> {
    let _ = (db, cmd);
    Err(AppError::not_implemented(
        "sync_use_case::trigger_contact_sync",
    ))
}

/// 读取账户及各跟踪文件夹的本地邮件同步状态、错误和最近成功时间。
///
/// 不触发同步，也不表示后台会话聚合已完成；账户不存在时返回未找到错误。
pub async fn get_mail_sync_progress(
    db: &SqlitePool,
    account_id: &AccountId,
) -> Result<MailSyncProgress, AppError> {
    let mut conn = db.acquire().await?;
    let account = accounts::get(&mut conn, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    let folders = folder_states::list_by_account(&mut conn, account_id).await?;
    Ok(MailSyncProgress {
        account_id: account_id.clone(),
        mail_sync_status: account.mail_sync_status,
        mail_sync_error: account.mail_sync_error,
        mail_synced_at: account.mail_synced_at,
        folders,
    })
}

/// 读取账户最近一次联系人同步成功时间及增量游标。
///
/// 当前读模型不包含运行中或失败状态，不能据此推断同步仍在进行。
/// 账户不存在时返回未找到错误。
pub async fn get_contact_sync_progress(
    db: &SqlitePool,
    account_id: &AccountId,
) -> Result<ContactSyncProgress, AppError> {
    let mut conn = db.acquire().await?;
    let account = accounts::get(&mut conn, account_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("账户不存在: {account_id}")))?;
    Ok(ContactSyncProgress {
        account_id: account_id.clone(),
        contact_synced_at: account.contact_synced_at,
        contact_delta_link: account.contact_delta_link,
    })
}

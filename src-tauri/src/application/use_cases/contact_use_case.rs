//! 联系人用例
use crate::infrastructure::repository::contacts;
use sqlx::SqlitePool;

use crate::application::dto::{ContactListQuery, CursorPage};
use crate::application::error::AppError;
use crate::domain::entities::Contact;
use crate::domain::value_objects::{AccountId, ContactId, EmailAddress};

/// 分页读取指定账户的联系人，可按显示名或邮箱地址前缀查找。
///
/// 游标及分页大小来自查询参数；不发起远端通讯录同步。
pub async fn list_contacts(
    db: &SqlitePool,
    query: ContactListQuery,
) -> Result<CursorPage<Contact>, AppError> {
    let mut conn = db.acquire().await?;
    contacts::list(&mut conn, &query).await.map_err(Into::into)
}

/// 在指定账户内按本地联系人 ID 查询；不存在时返回未找到错误。
pub async fn get_contact(
    db: &SqlitePool,
    account_id: &AccountId,
    contact_id: &ContactId,
) -> Result<Contact, AppError> {
    let mut conn = db.acquire().await?;
    contacts::get(&mut conn, account_id, contact_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("联系人不存在: {contact_id}")))
}

/// 在指定账户内按规范化邮箱地址查找联系人，未匹配时返回 `None`。
///
/// 仅查询已有联系人，不创建占位联系人或会话。
pub async fn get_contact_by_address(
    db: &SqlitePool,
    account_id: &AccountId,
    address: &EmailAddress,
) -> Result<Option<Contact>, AppError> {
    let mut conn = db.acquire().await?;
    contacts::get_by_address(&mut conn, account_id, address)
        .await
        .map_err(Into::into)
}

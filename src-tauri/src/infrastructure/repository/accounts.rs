//! SQLite accounts 模块函数；连接与事务由调用方持有。

use crate::infrastructure::error::InfraError;
use crate::domain::entities::Account;
use crate::domain::value_objects::{AccountId, EmailAddress, SyncStatus, TimestampMs};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

pub async fn get(
    conn: &mut SqliteConnection,
    id: &AccountId,
) -> Result<Option<Account>, InfraError> {
    let row = sqlx::query("SELECT * FROM accounts WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::account)
        .transpose()
        .map_err(Into::into)
}

pub async fn get_by_email(
    conn: &mut SqliteConnection,
    email: &EmailAddress,
) -> Result<Option<Account>, InfraError> {
    let row = sqlx::query("SELECT * FROM accounts WHERE email = ?")
        .bind(email.as_str())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::account)
        .transpose()
        .map_err(Into::into)
}

pub async fn get_by_home_account_id(
    conn: &mut SqliteConnection,
    home_account_id: &str,
) -> Result<Option<Account>, InfraError> {
    let row = sqlx::query("SELECT * FROM accounts WHERE home_account_id = ?")
        .bind(home_account_id)
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref()
        .map(super::rows::account)
        .transpose()
        .map_err(Into::into)
}

pub async fn list(conn: &mut SqliteConnection) -> Result<Vec<Account>, InfraError> {
    sqlx::query("SELECT * FROM accounts WHERE is_signed_in = 1 ORDER BY id")
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(super::rows::account)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// 全部账户行（含已退出）；别名冲突校验用。
pub async fn list_all(conn: &mut SqliteConnection) -> Result<Vec<Account>, InfraError> {
    sqlx::query("SELECT * FROM accounts ORDER BY id")
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(super::rows::account)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// 「全部账号」列表用的账号 ID：已登录且 `include_in_all`。
///
/// 退出只清凭证并把 `is_signed_in=0`；未退出也可关掉 `include_in_all` 而不进跨账号扫描。
/// 指定单账号过滤时不走此集合（见 [`resolve_list_account_ids`]）。
pub async fn list_ids(conn: &mut SqliteConnection) -> Result<Vec<AccountId>, InfraError> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM accounts WHERE is_signed_in = 1 AND include_in_all = 1 ORDER BY id",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(ids.into_iter().map(AccountId::new).collect())
}

/// 列表过滤：指定账号则单元素（不受 `include_in_all` 影响）；否则取「全部」集合。
pub async fn resolve_list_account_ids(
    conn: &mut SqliteConnection,
    account_id: Option<&AccountId>,
) -> Result<Vec<AccountId>, InfraError> {
    if let Some(id) = account_id {
        return Ok(vec![id.clone()]);
    }
    list_ids(conn).await
}

/// 向 QueryBuilder 追加 `account_id IN (...)`。
pub fn push_account_id_in<'qb>(
    qb: &mut QueryBuilder<'qb, Sqlite>,
    account_ids: &[AccountId],
) {
    qb.push("account_id IN (");
    let mut separated = qb.separated(", ");
    for id in account_ids {
        separated.push_bind(id.as_str().to_owned());
    }
    separated.push_unseparated(")");
}

/// 登录路径：插入新账户，或只更新身份字段（不覆盖显示开关与同步元数据）。
pub async fn upsert_identity(
    conn: &mut SqliteConnection,
    account: &Account,
) -> Result<(), InfraError> {
    let aliases_json = serde_json::to_string(
        &account
            .aliases
            .iter()
            .map(|a| a.as_str())
            .collect::<Vec<_>>(),
    )
    .map_err(|e| InfraError::Storage {
        message: format!("aliases_json 序列化失败: {e}"),
    })?;
    sqlx::query(
        "INSERT INTO accounts (
            id, email, display_name, aliases_json, tenant_id, home_account_id,
            contact_delta_link, mail_sync_triggered_at, contact_synced_at, mail_sync_status,
            mail_sync_error, is_signed_in, include_in_all, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, 1, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            email=excluded.email,
            display_name=excluded.display_name,
            aliases_json=excluded.aliases_json,
            tenant_id=excluded.tenant_id,
            home_account_id=excluded.home_account_id,
            is_signed_in=1,
            updated_at=excluded.updated_at",
    )
    .bind(account.id.as_str())
    .bind(account.email.as_str())
    .bind(&account.display_name)
    .bind(&aliases_json)
    .bind(&account.tenant_id)
    .bind(&account.home_account_id)
    .bind(&account.contact_delta_link)
    .bind(account.mail_sync_triggered_at)
    .bind(account.contact_synced_at)
    .bind(account.mail_sync_status.as_str())
    .bind(&account.mail_sync_error)
    .bind(account.created_at)
    .bind(account.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn upsert(conn: &mut SqliteConnection, account: &Account) -> Result<(), InfraError> {
    let aliases_json = serde_json::to_string(
        &account
            .aliases
            .iter()
            .map(|a| a.as_str())
            .collect::<Vec<_>>(),
    )
    .map_err(|e| InfraError::Storage {
        message: format!("aliases_json 序列化失败: {e}"),
    })?;
    sqlx::query(
        "INSERT INTO accounts (
            id, email, display_name, aliases_json, tenant_id, home_account_id,
            contact_delta_link, mail_sync_triggered_at, contact_synced_at, mail_sync_status,
            mail_sync_error, is_signed_in, include_in_all, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)
        ON CONFLICT(id) DO UPDATE SET
            email=excluded.email,
            display_name=excluded.display_name,
            aliases_json=excluded.aliases_json,
            tenant_id=excluded.tenant_id,
            home_account_id=excluded.home_account_id,
            contact_delta_link=excluded.contact_delta_link,
            mail_sync_triggered_at=excluded.mail_sync_triggered_at,
            contact_synced_at=excluded.contact_synced_at,
            mail_sync_status=excluded.mail_sync_status,
            mail_sync_error=excluded.mail_sync_error,
            is_signed_in=1,
            include_in_all=excluded.include_in_all,
            updated_at=excluded.updated_at",
    )
    .bind(account.id.as_str())
    .bind(account.email.as_str())
    .bind(&account.display_name)
    .bind(&aliases_json)
    .bind(&account.tenant_id)
    .bind(&account.home_account_id)
    .bind(&account.contact_delta_link)
    .bind(account.mail_sync_triggered_at)
    .bind(account.contact_synced_at)
    .bind(account.mail_sync_status.as_str())
    .bind(&account.mail_sync_error)
    .bind(account.include_in_all)
    .bind(account.created_at)
    .bind(account.updated_at)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 标记账号是否处于已登录会话（退出=0，登录=1）；不删账户行与邮件。
pub async fn set_signed_in(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    signed_in: bool,
) -> Result<(), InfraError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let result = sqlx::query(
        "UPDATE accounts SET is_signed_in = ?, updated_at = ? WHERE id = ?",
    )
    .bind(signed_in)
    .bind(now)
    .bind(account_id.as_str())
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(InfraError::Conflict {
            message: format!("账户不存在: {account_id}"),
        });
    }
    Ok(())
}

/// 进程启动时清掉遗留的 running，避免 UI 永久显示「同步中」。
pub async fn clear_stale_mail_sync_running(
    conn: &mut SqliteConnection,
) -> Result<u64, InfraError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let result = sqlx::query(
        "UPDATE accounts
         SET mail_sync_status = 'idle', mail_sync_error = NULL, updated_at = ?
         WHERE mail_sync_status = 'running'",
    )
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected())
}

pub async fn update_mail_sync_meta(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    status: SyncStatus,
    error: Option<&str>,
    // 传入则更新「发起同步」时间；成功结束勿传成功时刻
    mail_sync_triggered_at: Option<TimestampMs>,
) -> Result<(), InfraError> {
    sqlx::query(
        "UPDATE accounts SET mail_sync_status = ?, mail_sync_error = ?, mail_sync_triggered_at = COALESCE(?, mail_sync_triggered_at), updated_at = ?
         WHERE id = ?",
    )
    .bind(status.as_str())
    .bind(error)
    .bind(mail_sync_triggered_at)
    .bind(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    )
    .bind(account_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn update_contact_sync_meta(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    contact_delta_link: Option<&str>,
    contact_synced_at: Option<TimestampMs>,
) -> Result<(), InfraError> {
    sqlx::query(
        "UPDATE accounts SET contact_delta_link = ?, contact_synced_at = COALESCE(?, contact_synced_at), updated_at = ?
         WHERE id = ?",
    )
    .bind(contact_delta_link)
    .bind(contact_synced_at)
    .bind(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    )
    .bind(account_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn update_display_settings(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    include_in_all: bool,
) -> Result<(), InfraError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let result = sqlx::query(
        "UPDATE accounts SET include_in_all = ?, updated_at = ? WHERE id = ?",
    )
    .bind(include_in_all)
    .bind(now)
    .bind(account_id.as_str())
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(InfraError::Conflict {
            message: format!("账户不存在: {account_id}"),
        });
    }
    Ok(())
}

pub async fn update_aliases(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    aliases: &[EmailAddress],
) -> Result<(), InfraError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let aliases_json = serde_json::to_string(
        &aliases
            .iter()
            .map(|a| a.as_str())
            .collect::<Vec<_>>(),
    )
    .map_err(|e| InfraError::Storage {
        message: format!("aliases_json 序列化失败: {e}"),
    })?;
    let result = sqlx::query(
        "UPDATE accounts SET aliases_json = ?, updated_at = ? WHERE id = ?",
    )
    .bind(&aliases_json)
    .bind(now)
    .bind(account_id.as_str())
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(InfraError::Conflict {
            message: format!("账户不存在: {account_id}"),
        });
    }
    Ok(())
}

pub async fn delete(conn: &mut SqliteConnection, id: &AccountId) -> Result<(), InfraError> {
    sqlx::query("DELETE FROM accounts WHERE id = ?")
        .bind(id.as_str())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// 清除账号下聊天/同步本地数据；保留 `accounts` 行、登录态与 `account_mail_transport`。
///
/// 删除顺序尊重 FK：消息 → 会话 → 群/联系人；文件夹行保留勾选，仅重置游标。
pub async fn clear_local_chat_data(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<(), InfraError> {
    let id = account_id.as_str();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    // recipients / jobs / mutations 随 messages ON DELETE CASCADE
    sqlx::query("DELETE FROM messages WHERE account_id = ?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE sessions SET merged_into_id = NULL, contact_id = NULL, last_message_id = NULL,
         last_message_at = NULL, unread_count = 0 WHERE account_id = ?",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM sessions WHERE account_id = ?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    // group_participants 随 groups CASCADE
    sqlx::query("DELETE FROM groups WHERE account_id = ?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM contacts WHERE account_id = ?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE mail_folder_sync_state
         SET sync_cursor = NULL, page_cursor = NULL, synced_at = NULL,
             sync_status = 'idle', last_error = NULL, updated_at = ?
         WHERE account_id = ?",
    )
    .bind(now)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE accounts
         SET contact_delta_link = NULL,
             mail_sync_triggered_at = NULL,
             contact_synced_at = NULL,
             mail_sync_status = 'idle',
             mail_sync_error = NULL,
             updated_at = ?
         WHERE id = ?",
    )
    .bind(now)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}


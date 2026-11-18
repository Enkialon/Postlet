//! SQLite folder_states 模块函数；连接与事务由调用方持有。

use crate::infrastructure::error::InfraError;
use crate::domain::entities::MailFolderSyncState;
use crate::domain::value_objects::{AccountId, FolderId, ReceiveProtocol, SyncStatus};
use crate::domain::mail::DiscoveredFolder;
use sqlx::SqliteConnection;

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<Option<MailFolderSyncState>, InfraError> {
    let row = sqlx::query(
        "SELECT * FROM mail_folder_sync_state WHERE account_id = ? AND folder_id = ?",
    )
    .bind(account_id.as_str())
    .bind(folder_id.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref()
        .map(super::rows::mailfoldersyncstate)
        .transpose()
        .map_err(Into::into)
}

pub async fn list_by_account(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<Vec<MailFolderSyncState>, InfraError> {
    sqlx::query(
        "SELECT * FROM mail_folder_sync_state WHERE account_id = ? AND is_retired = 0
         ORDER BY display_name COLLATE NOCASE, folder_id",
    )
    .bind(account_id.as_str())
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(super::rows::mailfoldersyncstate)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

pub async fn list_sync_enabled(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<Vec<MailFolderSyncState>, InfraError> {
    sqlx::query(
        "SELECT * FROM mail_folder_sync_state
         WHERE account_id = ? AND sync_enabled = 1 AND is_retired = 0
         ORDER BY folder_id",
    )
    .bind(account_id.as_str())
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .map(super::rows::mailfoldersyncstate)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

pub async fn upsert(
    conn: &mut SqliteConnection,
    state: &MailFolderSyncState,
) -> Result<(), InfraError> {
    sqlx::query(
        "INSERT INTO mail_folder_sync_state (
            account_id, folder_id, parent_folder_id, display_name, sync_enabled,
            sync_cursor, page_cursor, synced_at, sync_status, last_error, updated_at, generation, protocol, remote_binding, binding_revision, is_retired,
            role, role_source
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(account_id, folder_id) DO UPDATE SET
            protocol=excluded.protocol,
            remote_binding=excluded.remote_binding,
            binding_revision=excluded.binding_revision,
            is_retired=excluded.is_retired,
            parent_folder_id=excluded.parent_folder_id,
            display_name=excluded.display_name,
            sync_enabled=excluded.sync_enabled,
            generation=excluded.generation,
            sync_cursor=excluded.sync_cursor,
            page_cursor=excluded.page_cursor,
            synced_at=excluded.synced_at,
            sync_status=excluded.sync_status,
            last_error=excluded.last_error,
            updated_at=excluded.updated_at,
            role=CASE WHEN mail_folder_sync_state.role_source = 'manual' THEN mail_folder_sync_state.role ELSE excluded.role END,
            role_source=CASE WHEN mail_folder_sync_state.role_source = 'manual' THEN 'manual' ELSE excluded.role_source END",
    )
    .bind(state.account_id.as_str())
    .bind(state.folder_id.as_str())
    .bind(state.parent_folder_id.as_ref().map(|f| f.as_str()))
    .bind(&state.display_name)
    .bind(if state.sync_enabled { 1 } else { 0 })
    .bind(&state.sync_cursor)
    .bind(&state.page_cursor)
    .bind(state.synced_at)
    .bind(state.sync_status.as_str())
    .bind(&state.last_error)
    .bind(state.updated_at)
    .bind(&state.generation)
    .bind(state.protocol.as_str())
    .bind(&state.remote_binding)
    .bind(state.binding_revision)
    .bind(state.is_retired)
    .bind(state.role.as_str())
    .bind(state.role_source.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 完整目录快照：按有效绑定复用本地身份；已退役的同名绑定永不复用。
/// 返回本次退役的 FolderId，调用方在同一事务失效其消息和任务。
pub async fn reconcile_discovered(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    protocol: ReceiveProtocol,
    folders: &[DiscoveredFolder],
    now: i64,
) -> Result<Vec<FolderId>, InfraError> {
    use std::collections::{HashMap, HashSet};
    let mut bindings = HashSet::new();
    for folder in folders {
        if folder.remote_binding.is_empty() || !bindings.insert(folder.remote_binding.as_str()) {
            return Err(InfraError::Conflict { message: "目录快照包含空绑定或重复绑定".into() });
        }
    }
    let active = list_by_account(conn, account_id).await?;
    let mut states = HashMap::new();
    for folder in folders {
        let state = active.iter().find(|state| state.protocol == protocol && state.remote_binding == folder.remote_binding)
            .cloned().unwrap_or_else(|| MailFolderSyncState {
                account_id: account_id.clone(), folder_id: FolderId::new(uuid::Uuid::new_v4().to_string()),
                protocol, remote_binding: folder.remote_binding.clone(), binding_revision: 1, is_retired: false,
                parent_folder_id: None, display_name: folder.display_name.clone(), sync_enabled: false,
                role: crate::domain::services::resolve_role(folder.server_role, None, &folder.display_name).0,
                role_source: crate::domain::services::resolve_role(folder.server_role, None, &folder.display_name).1,
                generation: None, sync_cursor: None, page_cursor: None, synced_at: None,
                sync_status: SyncStatus::Idle, last_error: None, updated_at: now,
            });
        states.insert(folder.remote_binding.clone(), state);
    }
    let identities: HashMap<_, _> = states.iter().map(|(binding, state)| (binding.clone(), state.folder_id.clone())).collect();
    for folder in folders {
        let state = states.get_mut(&folder.remote_binding).unwrap();
        state.parent_folder_id = folder.parent_binding.as_ref().and_then(|binding| identities.get(binding)).cloned();
        state.display_name = folder.display_name.clone(); state.updated_at = now;
        // 服务器角色可覆盖 guess；manual 由 upsert 的 CASE 保持
        let (role, source) = crate::domain::services::resolve_role(
            folder.server_role,
            if state.role_source == crate::domain::services::RoleSource::Manual { Some(state.role) } else { None },
            &folder.display_name,
        );
        state.role = role;
        state.role_source = source;
        upsert(conn, state).await?;
    }
    let mut retired = Vec::new();
    for old in active {
        if old.protocol != protocol || !bindings.contains(old.remote_binding.as_str()) {
            retire(conn, account_id, &old.folder_id, now).await?;
            retired.push(old.folder_id);
        }
    }
    Ok(retired)
}

pub async fn retire(conn: &mut SqliteConnection, account_id: &AccountId, folder_id: &FolderId, now: i64) -> Result<(), InfraError> {
    sqlx::query("UPDATE mail_folder_sync_state SET is_retired = 1, sync_enabled = 0, binding_revision = binding_revision + 1, sync_cursor = NULL, page_cursor = NULL, generation = NULL, updated_at = ? WHERE account_id = ? AND folder_id = ? AND is_retired = 0")
        .bind(now).bind(account_id.as_str()).bind(folder_id.as_str()).execute(conn).await?;
    Ok(())
}

pub async fn set_sync_enabled(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
    enabled: bool,
    now: i64,
) -> Result<(), InfraError> {
    let result = sqlx::query(
        "UPDATE mail_folder_sync_state
         SET sync_enabled = ?, updated_at = ?
         WHERE account_id = ? AND folder_id = ? AND is_retired = 0",
    )
    .bind(if enabled { 1 } else { 0 })
    .bind(now)
    .bind(account_id.as_str())
    .bind(folder_id.as_str())
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 0 {
        return Err(InfraError::Storage {
            message: format!("文件夹不存在: {folder_id}"),
        });
    }
    Ok(())
}

pub async fn reset_cursors(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<(), InfraError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    sqlx::query(
        "UPDATE mail_folder_sync_state
         SET sync_cursor = NULL, page_cursor = NULL, updated_at = ?
         WHERE account_id = ? AND folder_id = ?",
    )
    .bind(now)
    .bind(account_id.as_str())
    .bind(folder_id.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

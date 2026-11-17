//! 附件内容导入、读取与引用回收；持账号附件锁覆盖文件安装与 SQL 引用提交。

use crate::application::dto::{ComposeAttachmentDraft, ImportAttachmentCommand};
use crate::application::error::{AppError, AppErrorCode};
use crate::domain::value_objects::{AccountId, TimestampMs};
use crate::infrastructure::attachment_store::{self, AttachmentLock};
use crate::infrastructure::error::InfraError;
use crate::infrastructure::paths::AppPaths;
use crate::infrastructure::repository::{accounts, attachments};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// 运行中内容持有者（读取/发送中），回收前不可删除。
fn holders() -> &'static Mutex<HashMap<String, u32>> {
    static HOLDERS: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
    HOLDERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn holder_key(account_id: &AccountId, content_id: &str) -> String {
    format!("{}:{content_id}", account_id.as_str())
}

fn now_ms() -> TimestampMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn map_lock_err(err: InfraError) -> AppError {
    match err {
        InfraError::Conflict { message } => AppError {
            code: AppErrorCode::Conflict,
            message,
            retryable: true,
        },
        other => other.into(),
    }
}

/// 标记内容正在被运行中任务持有。
pub fn acquire_holder(account_id: &AccountId, content_id: &str) {
    let mut guard = holders().lock().unwrap_or_else(|e| e.into_inner());
    *guard.entry(holder_key(account_id, content_id)).or_insert(0) += 1;
}

/// 释放运行中持有。
pub fn release_holder(account_id: &AccountId, content_id: &str) {
    let mut guard = holders().lock().unwrap_or_else(|e| e.into_inner());
    let key = holder_key(account_id, content_id);
    if let Some(count) = guard.get_mut(&key) {
        *count = count.saturating_sub(1);
        if *count == 0 {
            guard.remove(&key);
        }
    }
}

fn is_held(account_id: &AccountId, content_id: &str) -> bool {
    let guard = holders().lock().unwrap_or_else(|e| e.into_inner());
    guard.get(&holder_key(account_id, content_id)).copied().unwrap_or(0) > 0
}

/// 导入附件：临时写入 → 校验安装 → 持久草稿引用；持锁覆盖全过程。
pub async fn import_attachment(
    db: &SqlitePool,
    paths: &AppPaths,
    cmd: ImportAttachmentCommand,
) -> Result<ComposeAttachmentDraft, AppError> {
    let bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        cmd.content_base64.trim(),
    )
    .map_err(|_| AppError::validation("附件内容 Base64 无效"))?;

    if bytes.len() as i64 != cmd.byte_length {
        return Err(AppError::validation("附件长度与声明不一致"));
    }
    let name = cmd.name.trim();
    if name.is_empty() {
        return Err(AppError::validation("附件名称不能为空"));
    }
    let mime = if cmd.mime.trim().is_empty() {
        "application/octet-stream".to_string()
    } else {
        cmd.mime.trim().to_string()
    };

    {
        let mut conn = db.acquire().await?;
        accounts::get(&mut conn, &cmd.account_id)
            .await?
            .ok_or_else(|| AppError::not_found(format!("账户不存在: {}", cmd.account_id)))?;
    }

    let _lock = attachment_store::lock_account(&paths.attachments_dir, &cmd.account_id)
        .map_err(map_lock_err)?;

    let (temp_path, digest) =
        attachment_store::write_temp(&paths.attachments_dir, &cmd.account_id, &bytes)
            .map_err(AppError::from)?;
    if digest != hex::encode(Sha256::digest(&bytes)) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(AppError::internal("附件摘要计算不一致"));
    }

    let content_id = Uuid::new_v4().to_string();
    let storage_key = content_id.clone();
    let install_result = attachment_store::install_from_temp(
        &paths.attachments_dir,
        &cmd.account_id,
        &temp_path,
        &storage_key,
        bytes.len() as u64,
        &digest,
    );
    if let Err(err) = install_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(err.into());
    }

    let now = now_ms();
    let att = ComposeAttachmentDraft {
        content_id: content_id.clone(),
        name: name.to_string(),
        size: bytes.len() as i64,
        mime,
    };

    let mut tx = db.begin().await?;
    let persist = async {
        let draft_id = attachments::ensure_draft(
            &mut tx,
            &cmd.account_id,
            &cmd.session_id,
            now,
        )
        .await?;
        attachments::insert_content(
            &mut tx,
            &attachments::AttachmentContentRow {
                content_id: content_id.clone(),
                account_id: cmd.account_id.clone(),
                storage_key: storage_key.clone(),
                byte_length: bytes.len() as i64,
                digest: digest.clone(),
                created_at: now,
            },
        )
        .await?;
        let sort_order =
            attachments::next_draft_attachment_order(&mut tx, &cmd.account_id, &draft_id).await?;
        attachments::insert_draft_attachment(
            &mut tx,
            &cmd.account_id,
            &draft_id,
            &attachments::AttachmentRefRow {
                content_id: att.content_id.clone(),
                name: att.name.clone(),
                mime: att.mime.clone(),
                sort_order,
                byte_length: att.size,
            },
        )
        .await?;
        Ok::<_, InfraError>(())
    }
    .await;

    match persist {
        Ok(()) => {
            tx.commit().await?;
            Ok(att)
        }
        Err(err) => {
            let _ = tx.rollback().await;
            let _ = attachment_store::delete_object(
                &paths.attachments_dir,
                &cmd.account_id,
                &storage_key,
            );
            Err(err.into())
        }
    }
}

/// 读取内容字节；缺失时返回可见错误。运行中持有保护期间不会被回收删除。
pub async fn load_content(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    content_id: &str,
) -> Result<Vec<u8>, AppError> {
    let mut conn = db.acquire().await?;
    let row = attachments::get_content(&mut conn, account_id, content_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("附件内容不存在: {content_id}")))?;
    drop(conn);

    acquire_holder(account_id, content_id);
    let result = attachment_store::load_bytes(&paths.attachments_dir, account_id, &row.storage_key);
    release_holder(account_id, content_id);
    result.map_err(AppError::from)
}

/// 回收无引用内容；必须在已持有账号附件锁时调用，或由本函数自行取锁。
pub async fn recycle_unreferenced(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
    existing_lock: Option<&AttachmentLock>,
) -> Result<usize, AppError> {
    let _owned_lock = if existing_lock.is_none() {
        Some(
            attachment_store::lock_account(&paths.attachments_dir, account_id)
                .map_err(map_lock_err)?,
        )
    } else {
        None
    };

    let mut conn = db.acquire().await?;
    let candidates = attachments::collect_unreferenced(&mut conn, account_id).await?;
    drop(conn);

    // 过滤运行中持有者
    let to_delete: Vec<_> = candidates
        .into_iter()
        .filter(|c| !is_held(account_id, &c.content_id))
        .collect();

    let mut removed = 0usize;
    for row in &to_delete {
        if is_held(account_id, &row.content_id) {
            continue;
        }
        let mut conn = db.acquire().await?;
        let still_unreferenced = attachments::collect_unreferenced(&mut conn, account_id)
            .await?
            .into_iter()
            .any(|c| c.content_id == row.content_id);
        if !still_unreferenced {
            continue;
        }
        attachment_store::delete_object(&paths.attachments_dir, account_id, &row.storage_key)
            .map_err(AppError::from)?;
        attachments::delete_content(&mut conn, account_id, &row.content_id).await?;
        removed += 1;
        drop(conn);
    }

    // 孤儿文件：磁盘有、SQL 无
    let mut conn = db.acquire().await?;
    let known = attachments::list_contents(&mut conn, account_id).await?;
    drop(conn);
    let known_keys: std::collections::HashSet<_> =
        known.into_iter().map(|c| c.storage_key).collect();
    for key in attachment_store::list_object_keys(&paths.attachments_dir, account_id)
        .map_err(AppError::from)?
    {
        if !known_keys.contains(&key) {
            let _ = attachment_store::delete_object(&paths.attachments_dir, account_id, &key);
        }
    }

    // SQL 有、磁盘无：保留行供上层可见错误；启动核对可继续清理无引用行
    let mut conn = db.acquire().await?;
    let dangling = attachments::collect_unreferenced(&mut conn, account_id).await?;
    for row in dangling {
        if !attachment_store::object_exists(&paths.attachments_dir, account_id, &row.storage_key)
            && !is_held(account_id, &row.content_id)
        {
            attachments::delete_content(&mut conn, account_id, &row.content_id).await?;
        }
    }
    drop(conn);

    attachment_store::clean_temp_dir(&paths.attachments_dir, account_id).map_err(AppError::from)?;
    Ok(removed)
}

/// 启动时核对：清理暂存、孤儿文件与无引用行。
pub async fn reconcile_account(
    db: &SqlitePool,
    paths: &AppPaths,
    account_id: &AccountId,
) -> Result<(), AppError> {
    let _lock = match attachment_store::lock_account(&paths.attachments_dir, account_id) {
        Ok(lock) => lock,
        Err(InfraError::Conflict { .. }) => return Ok(()), // 忙则跳过，下次再核
        Err(err) => return Err(err.into()),
    };
    recycle_unreferenced(db, paths, account_id, Some(&_lock)).await?;
    Ok(())
}

/// 清除账号附件目录（本地数据清理后调用；调用方应已删 SQL 引用）。
pub fn purge_account_files(attachments_root: &Path, account_id: &AccountId) -> Result<(), AppError> {
    let dir = attachment_store::account_dir(attachments_root, account_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|_| AppError::internal("清理附件目录失败"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{import_attachment, load_content, recycle_unreferenced};
    use crate::application::dto::{
        ComposeAttachmentDraft, ImportAttachmentCommand, SaveComposeDraftCommand,
    };
    use crate::application::error::AppErrorCode;
    use crate::application::use_cases::compose_use_case;
    use crate::domain::entities::Session;
    use crate::domain::value_objects::{AccountId, EmailAddress, SessionId, SessionKind};
    use crate::infrastructure::attachment_store;
    use crate::infrastructure::db;
    use crate::infrastructure::paths::AppPaths;
    use crate::infrastructure::repository::{attachments, sessions};
    use base64::Engine;
    use sqlx::SqlitePool;
    use std::path::Path;

    fn test_paths(root: &Path) -> AppPaths {
        let p = AppPaths {
            config_dir: root.join("config"),
            local_data_dir: root.to_owned(),
            data_dir: root.join("data"),
            attachments_dir: root.join("attachments"),
            credentials_dir: root.join("credentials"),
            database_path: root.join("data/test.db"),
        };
        for dir in [
            &p.config_dir,
            &p.data_dir,
            &p.attachments_dir,
            &p.credentials_dir,
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        p
    }

    async fn seed(db: &SqlitePool) {
        sqlx::query(
            "INSERT INTO accounts (id, email, display_name, aliases_json, mail_sync_status, created_at, updated_at)
             VALUES ('a', 'a@example.com', 'a', '[]', 'idle', 1, 1)",
        )
        .execute(db)
        .await
        .unwrap();
        let mut conn = db.acquire().await.unwrap();
        sessions::upsert(
            &mut conn,
            &Session {
                id: SessionId::new("s1"),
                account_id: AccountId::new("a"),
                kind: SessionKind::Private,
                normalized_address: Some(EmailAddress::parse("peer@example.com").unwrap()),
                contact_id: None,
                group_id: None,
                merged_into_id: None,
                last_message_at: None,
                last_message_id: None,
                unread_count: 0,
                hidden_at: None,
                created_at: 1,
                updated_at: 1,
            },
        )
        .await
        .unwrap();
    }

    // 确定性验证：回收一旦持有账号附件锁，并发 save 必须得到可重试冲突，
    // 不能把「有效引用」写到「已删除内容」上。
    #[tokio::test]
    async fn recycle_lock_blocks_concurrent_re_reference() {
        let root = tempfile::tempdir().unwrap();
        let paths = test_paths(root.path());
        let db = db::open_memory().await.unwrap();
        seed(&db).await;

        let bytes = b"race-payload";
        let att = import_attachment(
            &db,
            &paths,
            ImportAttachmentCommand {
                account_id: AccountId::new("a"),
                session_id: SessionId::new("s1"),
                name: "race.bin".into(),
                mime: "application/octet-stream".into(),
                byte_length: bytes.len() as i64,
                content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            },
        )
        .await
        .unwrap();

        // 仅摘掉草稿引用（不走 save→recycle），留下无引用内容供后续回收
        {
            let mut tx = db.begin().await.unwrap();
            let draft = attachments::get_draft_by_session(
                &mut tx,
                &AccountId::new("a"),
                &SessionId::new("s1"),
            )
            .await
            .unwrap()
            .expect("draft");
            attachments::replace_draft_attachments(&mut tx, &AccountId::new("a"), &draft.draft_id, &[])
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }

        let account = AccountId::new("a");
        // 模拟回收持锁窗口：主线程先握住账号附件锁
        let held = attachment_store::lock_account(&paths.attachments_dir, &account)
            .expect("acquire recycle lock");

        let err = compose_use_case::save_draft_with_refs(
            &db,
            &paths,
            SaveComposeDraftCommand {
                account_id: AccountId::new("a"),
                session_id: SessionId::new("s1"),
                reply_to_message_id: None,
                to: vec![],
                cc: vec![],
                bcc: vec![],
                subject: "re-ref".into(),
                body_html: String::new(),
                attachments: vec![ComposeAttachmentDraft {
                    content_id: att.content_id.clone(),
                    name: "race.bin".into(),
                    size: bytes.len() as i64,
                    mime: "application/octet-stream".into(),
                }],
            },
        )
        .await
        .expect_err("save during recycle must conflict");
        assert_eq!(err.code, AppErrorCode::Conflict);
        assert!(err.retryable);

        drop(held);
        let removed = recycle_unreferenced(&db, &paths, &account, None)
            .await
            .unwrap();
        assert_eq!(removed, 1);

        let err = load_content(&db, &paths, &AccountId::new("a"), &att.content_id)
            .await
            .unwrap_err();
        assert_eq!(err.code, AppErrorCode::NotFound);
    }
}

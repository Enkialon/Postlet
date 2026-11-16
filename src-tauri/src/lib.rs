//! Postlet 库入口：DDD 模块 + Tauri IPC 薄胶水。

pub mod application;
pub mod commands;
pub mod domain;
pub mod infrastructure;
pub mod runtime;

use application::dto::ComposeDraft;
use application::use_cases::compose_use_case;
use infrastructure::paths::AppPaths;
use runtime::{ChangeDispatcher, MailSyncGate};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::Manager;

/// 进程级应用状态（本地 SQLite + 目录布局 + 写信草稿 + 变更分发）
pub struct AppState {
    pub db: SqlitePool,
    pub paths: AppPaths,
    /// 会话级写信草稿（本阶段进程内，不落库）
    pub compose_drafts: Mutex<HashMap<String, ComposeDraft>>,
    /// 事务提交后的 ChangeSet 分发（UI 事件 / 系统通知）
    pub changes: ChangeDispatcher,
    /// 账户级邮件同步互斥
    pub mail_sync_gate: MailSyncGate,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let paths = AppPaths::resolve(app.handle())?;
            let db =
                tauri::async_runtime::block_on(infrastructure::db::open_sqlite(&paths.database_path))
                    .map_err(|e| format!("打开数据库失败: {e}"))?;
            // 上次进程若在同步中退出，清掉遗留 running，避免 UI 永久「同步中」
            tauri::async_runtime::block_on(async {
                if let Ok(mut conn) = db.acquire().await {
                    let _ = infrastructure::repository::accounts::clear_stale_mail_sync_running(
                        &mut conn,
                    )
                    .await;
                }
            });
            let changes = ChangeDispatcher::new(app.handle().clone());
            let mail_sync_gate = MailSyncGate::new();
            runtime::mail_watcher::spawn_watchers(
                db.clone(),
                paths.clone(),
                changes.clone(),
                mail_sync_gate.clone(),
            );
            runtime::mail_sync_scheduler::spawn(
                db.clone(),
                paths.clone(),
                changes.clone(),
                mail_sync_gate.clone(),
            );
            runtime::aggregate_scheduler::spawn(db.clone(), changes.clone());
            app.manage(AppState {
                db,
                paths,
                compose_drafts: Mutex::new(compose_use_case::empty_draft_map()),
                changes,
                mail_sync_gate,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::get_account,
            commands::account_sign_in,
            commands::account_sign_out,
            commands::upsert_imap_smtp_account,
            commands::update_display_settings,
            commands::list_contacts,
            commands::get_contact,
            commands::get_contact_by_address,
            commands::list_conversations,
            commands::get_conversation,
            commands::list_conversation_messages,
            commands::hide_conversation,
            commands::get_message,
            commands::ensure_message_body,
            commands::mark_messages_read,
            commands::list_groups,
            commands::get_group,
            commands::list_group_participants,
            commands::get_mail_sync_progress,
            commands::trigger_mail_sync,
            commands::refresh_mail_folders,
            commands::set_folder_sync_enabled,
            commands::trigger_contact_sync,
            commands::get_contact_sync_progress,
            commands::ensure_contact_conversation,
            commands::ensure_group_conversation,
            commands::create_manual_group,
            commands::rename_group,
            commands::merge_groups,
            commands::move_thread_to_group,
            commands::update_group_send_members,
            commands::compose_get_draft,
            commands::compose_save_draft,
            commands::compose_clear_draft,
            commands::compose_send,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

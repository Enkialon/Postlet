//! Postlet 库入口：DDD 模块 + Tauri IPC 薄胶水。

pub mod application;
pub mod commands;
pub mod domain;
pub mod infrastructure;

use application::dto::ComposeDraft;
use application::use_cases::compose_use_case;
use infrastructure::paths::AppPaths;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::Manager;

/// 进程级应用状态（本地 SQLite + 目录布局 + 写信草稿）
pub struct AppState {
    pub db: SqlitePool,
    pub paths: AppPaths,
    /// 会话级写信草稿（本阶段进程内，不落库）
    pub compose_drafts: Mutex<HashMap<String, ComposeDraft>>,
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
            app.manage(AppState {
                db,
                paths,
                compose_drafts: Mutex::new(compose_use_case::empty_draft_map()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::get_account,
            commands::load_demo_workspace,
            commands::update_display_settings,
            commands::list_contacts,
            commands::get_contact,
            commands::get_contact_by_address,
            commands::list_conversations,
            commands::get_conversation,
            commands::list_conversation_messages,
            commands::get_message,
            commands::ensure_message_body,
            commands::mark_messages_read,
            commands::list_groups,
            commands::get_group,
            commands::list_group_participants,
            commands::get_mail_sync_progress,
            commands::get_contact_sync_progress,
            commands::compose_get_draft,
            commands::compose_save_draft,
            commands::compose_clear_draft,
            commands::compose_send,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

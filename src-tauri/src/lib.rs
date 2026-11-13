//! Postlet 库入口：DDD 模块 + Tauri IPC 薄胶水。

pub mod application;
pub mod commands;
pub mod domain;
pub mod infrastructure;

use sqlx::SqlitePool;
use tauri::Manager;

/// 进程级应用状态（本地 SQLite）。
pub struct AppState {
    pub db: SqlitePool,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("无法解析 app_data_dir: {e}"))?;
            std::fs::create_dir_all(&app_data)
                .map_err(|e| format!("无法创建 app_data_dir: {e}"))?;
            let db_path = app_data.join("postlet.db");
            let db = tauri::async_runtime::block_on(infrastructure::db::open_sqlite(&db_path))
                .map_err(|e| format!("打开数据库失败: {e}"))?;
            app.manage(AppState { db });
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

//! Postlet 库入口：DDD 模块 + Tauri IPC 薄胶水。

pub mod application;
pub mod commands;
pub mod domain;
pub mod infrastructure;
pub mod runtime;
pub mod query_page;

use infrastructure::paths::AppPaths;
use runtime::{ChangeDispatcher, MailSyncGate};
use sqlx::SqlitePool;
use tauri::Manager;

/// 进程级应用状态（本地 SQLite + 目录布局 + 变更分发）
pub struct AppState {
    pub db: SqlitePool,
    pub paths: AppPaths,
    /// 事务提交后的 ChangeSet 分发（UI 事件 / 系统通知）
    pub changes: ChangeDispatcher,
    /// 账户级邮件同步互斥
    pub mail_sync_gate: MailSyncGate,
    pub mail_watchers: runtime::mail_watchers::MailWatchers,
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
            tauri::async_runtime::block_on(
                application::use_cases::mail_account_use_case::recover_credentials(&db, &paths),
            ).map_err(|error| format!("恢复邮件凭据失败: {}", error.message))?;
            let mail_sync_gate = MailSyncGate::new();
            let mail_watchers = runtime::mail_watchers::MailWatchers::spawn(
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
            tauri::async_runtime::block_on(
                application::use_cases::message_use_case::recover_interrupted_mutations(&db),
            )
            .ok();
            runtime::aggregate_scheduler::spawn(db.clone(), changes.clone());
            runtime::archive_worker::spawn(
                db.clone(),
                paths.clone(),
                mail_sync_gate.clone(),
            );
            runtime::send_worker::spawn(
                db.clone(),
                paths.clone(),
                changes.clone(),
                mail_sync_gate.clone(),
            );
            // 启动核对：暂存/孤儿/无引用附件（忙锁则跳过单个账号）
            {
                let db_reconcile = db.clone();
                let paths_reconcile = paths.clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(mut conn) = db_reconcile.acquire().await {
                        if let Ok(ids) =
                            infrastructure::repository::accounts::list_ids(&mut conn).await
                        {
                            drop(conn);
                            for account_id in ids {
                                let _ = application::use_cases::attachment_use_case::reconcile_account(
                                    &db_reconcile,
                                    &paths_reconcile,
                                    &account_id,
                                )
                                .await;
                            }
                        }
                    }
                });
            }
            app.manage(AppState {
                db,
                paths,
                changes,
                mail_sync_gate,
                mail_watchers,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::get_account,
            commands::account_sign_in,
            commands::account_sign_out,
            commands::account_clear_local_data,
            commands::upsert_imap_smtp_account,
            commands::update_display_settings,
            commands::update_account_aliases,
            commands::update_sent_archive_policy,
            commands::get_account_change_version,
            commands::list_contacts,
            commands::get_contact,
            commands::get_contact_by_address,
            commands::list_sessions,
            commands::get_session,
            commands::list_session_messages,
            commands::hide_session,
            commands::get_message,
            commands::ensure_message_body,
            commands::mark_messages_read,
            commands::delete_messages,
            commands::list_groups,
            commands::get_group,
            commands::list_group_participants,
            commands::get_mail_sync_progress,
            commands::trigger_mail_sync,
            commands::refresh_mail_folders,
            commands::set_folder_sync_enabled,
            commands::trigger_contact_sync,
            commands::get_contact_sync_progress,
            commands::ensure_private_session,
            commands::ensure_group_session,
            commands::create_manual_group,
            commands::rename_group,
            commands::hide_group,
            commands::merge_sessions,
            commands::reassign_messages,
            commands::update_group_send_members,
            commands::compose_get_draft,
            commands::compose_save_draft,
            commands::compose_clear_draft,
            commands::compose_import_attachment,
            commands::compose_send,
            commands::get_send_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

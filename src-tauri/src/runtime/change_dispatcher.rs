//! 事务提交后的 ChangeSet 分发：UI 失效事件 +（按需）系统通知

use crate::application::dto::{ChangeOrigin, ChangeSet, MailChangedEvent};
use tauri::{AppHandle, Emitter};

/// 事件名：前端 `listen('mail-changed', …)`（Tauri 禁止 `.`）
pub const MAIL_CHANGED_EVENT: &str = "mail-changed";

/// 进程内变更分发器（组合根持有；用例不依赖 Tauri）
#[derive(Clone)]
pub struct ChangeDispatcher {
    app: AppHandle,
}

impl ChangeDispatcher {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    /// commit 成功后调用；空变更直接返回。
    pub fn dispatch(&self, changes: ChangeSet) {
        if changes.is_empty() {
            return;
        }

        let event = MailChangedEvent::from_change_set(&changes);
        if let Err(e) = self.app.emit(MAIL_CHANGED_EVENT, &event) {
            // 前端未监听时不应拖垮写路径
            eprintln!("mail-changed emit 失败: {e}");
        }

        maybe_notify_new_mail(&changes);
    }
}

/// 系统通知策略入口；V1 仅在 IncrementalSync + 有新入站时生效（插件接入前为 stub）。
fn maybe_notify_new_mail(changes: &ChangeSet) {
    if !matches!(changes.origin, ChangeOrigin::IncrementalSync) {
        return;
    }
    if changes.new_incoming_ids.is_empty() {
        return;
    }
    // TODO: tauri-plugin-notification — 汇总「N 封新邮件」、去重、免打扰与焦点策略
    let _ = changes.new_incoming_ids.len();
}

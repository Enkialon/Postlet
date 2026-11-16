//! 账户级邮件同步互斥：UI / 调度器 / IDLE 共用，避免重入把状态卡在 running。

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// 进程内同步闸门（可 Clone 进后台任务）。
#[derive(Clone, Default)]
pub struct MailSyncGate {
    inflight: Arc<Mutex<HashSet<String>>>,
}

impl MailSyncGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// 若该账户已在同步则返回 false；否则登记并返回 true。
    pub fn try_begin(&self, account_id: &str) -> bool {
        match self.inflight.lock() {
            Ok(mut guard) => guard.insert(account_id.to_string()),
            Err(_) => false,
        }
    }

    pub fn end(&self, account_id: &str) {
        if let Ok(mut guard) = self.inflight.lock() {
            guard.remove(account_id);
        }
    }
}

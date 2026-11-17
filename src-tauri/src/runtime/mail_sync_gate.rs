//! 账户级邮件同步互斥：UI / 调度器 / IDLE 共用，避免重入把状态卡在 running。

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// 进程内同步闸门（可 Clone 进后台任务）。
#[derive(Clone, Default)]
pub struct MailSyncGate {
    inflight: Arc<Mutex<HashSet<String>>>,
}

pub struct MailSyncPermit {
    gate: MailSyncGate,
    account_id: String,
}

impl Drop for MailSyncPermit {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.gate.inflight.lock() {
            guard.remove(&self.account_id);
        }
    }
}

impl MailSyncGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Permit 归实际任务所有；取消等待者不会提前释放它。
    pub fn try_acquire(&self, account_id: &str) -> Option<MailSyncPermit> {
        let mut guard = self.inflight.lock().ok()?;
        if !guard.insert(account_id.to_string()) {
            return None;
        }
        Some(MailSyncPermit {
            gate: self.clone(),
            account_id: account_id.to_string(),
        })
    }
}

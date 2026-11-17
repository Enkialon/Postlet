//! 账号同步互斥与配置变更屏障；Permit 由实际任务持有。
use crate::application::error::AppError;
use crate::infrastructure::mail::operation::MailOperation;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct GateState {
    inflight: HashMap<String, Option<MailOperation>>,
    paused: HashSet<String>,
}

#[derive(Clone, Default)]
pub struct MailSyncGate {
    state: Arc<Mutex<GateState>>,
    changed: Arc<tokio::sync::Notify>,
}

pub struct MailSyncPermit {
    gate: MailSyncGate,
    account_id: String,
}
impl Drop for MailSyncPermit {
    fn drop(&mut self) {
        self.gate
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .inflight
            .remove(&self.account_id);
        self.gate.changed.notify_waiters();
    }
}

pub struct MailSyncPause {
    gate: MailSyncGate,
    account_id: String,
}
impl Drop for MailSyncPause {
    fn drop(&mut self) {
        self.gate
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .paused
            .remove(&self.account_id);
        self.gate.changed.notify_waiters();
    }
}

impl MailSyncGate {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn try_acquire(&self, account_id: &str) -> Option<MailSyncPermit> {
        self.try_acquire_operation(account_id, None)
    }
    pub fn try_acquire_operation(
        &self,
        account_id: &str,
        operation: Option<MailOperation>,
    ) -> Option<MailSyncPermit> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.paused.contains(account_id) || state.inflight.contains_key(account_id) {
            return None;
        }
        state.inflight.insert(account_id.to_owned(), operation);
        Some(MailSyncPermit {
            gate: self.clone(),
            account_id: account_id.to_owned(),
        })
    }
    pub async fn pause_and_wait(&self, account_id: &str) -> Result<MailSyncPause, AppError> {
        let operation = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if !state.paused.insert(account_id.to_owned()) {
                return Err(AppError::conflict("账号配置正在变更，请稍后重试"));
            }
            state.inflight.get(account_id).cloned().flatten()
        };
        let pause = MailSyncPause {
            gate: self.clone(),
            account_id: account_id.to_owned(),
        };
        if let Some(operation) = operation {
            operation.cancel();
        }
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .inflight
                .contains_key(account_id)
            {
                break;
            }
            notified.await;
        }
        Ok(pause)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn account_transition_cancels_active_work_and_keeps_new_work_blocked_until_resume() {
        let gate = MailSyncGate::new();
        let operation = MailOperation::new(Duration::from_secs(5));
        let permit = gate
            .try_acquire_operation("a", Some(operation.clone()))
            .unwrap();
        let other = gate.clone();
        let waiter = tokio::spawn(async move { other.pause_and_wait("a").await.unwrap() });
        tokio::time::timeout(Duration::from_secs(1), operation.cancelled())
            .await
            .unwrap();
        assert!(!waiter.is_finished());
        assert!(gate.try_acquire("a").is_none());
        assert!(gate.try_acquire("b").is_some());
        drop(permit);
        let pause = waiter.await.unwrap();
        assert!(gate.try_acquire("a").is_none());
        assert!(gate.pause_and_wait("a").await.is_err());
        assert!(gate.try_acquire("a").is_none());
        drop(pause);
        assert!(gate.try_acquire("a").is_some());
    }

    #[tokio::test]
    async fn cancelling_transition_waiter_does_not_strand_pause_or_release_active_permit() {
        let gate = MailSyncGate::new();
        let operation = MailOperation::new(Duration::from_secs(5));
        let permit = gate
            .try_acquire_operation("a", Some(operation.clone()))
            .unwrap();
        let other = gate.clone();
        let waiter = tokio::spawn(async move { other.pause_and_wait("a").await });
        tokio::time::timeout(Duration::from_secs(1), operation.cancelled())
            .await
            .unwrap();
        waiter.abort();
        let _ = waiter.await;
        assert!(gate.try_acquire("a").is_none());
        drop(permit);
        assert!(gate.try_acquire("a").is_some());
    }
}

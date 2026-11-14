//! IMAP IDLE（阻塞）；不支持时由上层轮询。

use super::client::ImapSession;
use crate::application::error::PortError;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleOutcome {
    Changed,
    TimedOut,
    Disconnected,
}

/// 在已 SELECT 的邮箱上 IDLE，最长 `idle_timeout`。
pub fn run_idle_until_cancel(
    session: &mut ImapSession,
    idle_timeout: Duration,
) -> Result<IdleOutcome, PortError> {
    let mut idle = session.idle().map_err(|e| PortError::Remote {
        message: format!("IMAP IDLE 初始化失败: {e}"),
        retryable: true,
    })?;
    idle.set_keepalive(idle_timeout);
    match idle.wait_keepalive() {
        Ok(()) => Ok(IdleOutcome::Changed),
        Err(_) => Ok(IdleOutcome::Disconnected),
    }
}

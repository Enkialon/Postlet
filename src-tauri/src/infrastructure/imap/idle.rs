//! IMAP IDLE（阻塞）；不支持时由上层轮询。

use super::client::ImapSession;
use crate::application::error::PortError;
use imap::extensions::idle::WaitOutcome;
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
    let idle = session.idle().map_err(|e| PortError::Remote {
        message: format!("IMAP IDLE 初始化失败: {e}"),
        retryable: true,
    })?;
    eprintln!(
        "IMAP IDLE: 服务器已确认进入监听，等待变化（最长 {} 秒）",
        idle_timeout.as_secs()
    );
    match idle.wait_with_timeout(idle_timeout) {
        Ok(WaitOutcome::MailboxChanged) => Ok(IdleOutcome::Changed),
        Ok(WaitOutcome::TimedOut) => Ok(IdleOutcome::TimedOut),
        Err(error) => {
            eprintln!("IMAP IDLE: 等待失败，连接断开: {error}");
            Ok(IdleOutcome::Disconnected)
        }
    }
}

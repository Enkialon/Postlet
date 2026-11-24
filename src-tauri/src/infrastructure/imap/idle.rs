//! IMAP IDLE（阻塞）；不支持时由上层轮询。

use super::client::ImapSession;
use crate::infrastructure::error::InfraError;
use imap::extensions::idle::WaitOutcome;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleOutcome {
    Changed,
    TimedOut,
    Disconnected,
}

/// 在已 SELECT 的邮箱上 IDLE，最长 `idle_timeout`。
pub fn run_idle_until_cancel(
    session: &mut ImapSession,
    folder: &str,
    idle_timeout: Duration,
) -> Result<IdleOutcome, InfraError> {
    let idle = session.idle().map_err(|e| InfraError::Remote {
        message: format!("IMAP IDLE 初始化失败: {e}"),
        retryable: true,
    })?;
    let started = Instant::now();
    eprintln!(
        "[{}] IMAP IDLE {folder:?}: 服务器已确认监听（最长 {} 秒）",
        chrono::Local::now().format("%F %T%.3f"),
        idle_timeout.as_secs()
    );
    let (outcome, detail) = match idle.wait_with_timeout(idle_timeout) {
        Ok(WaitOutcome::MailboxChanged) => (IdleOutcome::Changed, "收到服务器变化通知，将请求同步（不一定是新邮件）".into()),
        Ok(WaitOutcome::TimedOut) => (IdleOutcome::TimedOut, "本轮监听超时，未收到变化通知".into()),
        Err(error) => (IdleOutcome::Disconnected, format!("等待失败，连接断开: {error}")),
    };
    eprintln!(
        "[{}] IMAP IDLE {folder:?}: {detail}，等待 {} ms",
        chrono::Local::now().format("%F %T%.3f"),
        started.elapsed().as_millis()
    );
    Ok(outcome)
}

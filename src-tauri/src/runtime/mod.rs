//! 进程内运行时：变更分发、邮件同步 / IDLE / 轮询调度。
//!
//! 调度与分发只依赖 Application DTO / 用例；不直接调用协议 SDK。

pub mod change_dispatcher;
pub mod mail_sync_scheduler;
pub mod mail_watcher;

pub use change_dispatcher::ChangeDispatcher;

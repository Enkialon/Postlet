//! 进程内运行时：变更分发、（后续）邮件同步 / 聚合调度。
//!
//! 调度与分发只依赖 Application DTO / 用例；不直接调用 `infrastructure::graph`。

pub mod change_dispatcher;

pub use change_dispatcher::ChangeDispatcher;

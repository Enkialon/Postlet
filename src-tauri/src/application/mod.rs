//! 应用层：用例编排，直接调用 Infrastructure 模块函数；不含 Graph SDK

pub mod dto;
pub mod error;
pub mod self_view;
pub mod use_cases;

pub use error::{AppError, AppErrorCode};

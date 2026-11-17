//! 纯领域层：无 graph-rs-sdk / tauri / 文件系统 / 网络

pub mod entities;
pub mod error;
pub mod services;
pub mod value_objects;

pub use error::DomainError;

pub mod mail;

//! 基础设施技术错误；应用层负责映射为用例错误。

/// 基础设施错误；不属于 Domain，禁止泄漏 SQL / Token / SDK 错误详情
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfraError {
    Storage { message: String },
    Auth { message: String },
    Network { message: String },
    Remote { message: String, retryable: bool },
    Conflict { message: String },
}

impl std::fmt::Display for InfraError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage { message } => write!(f, "存储故障: {message}"),
            Self::Auth { message } => write!(f, "鉴权故障: {message}"),
            Self::Network { message } => write!(f, "网络故障: {message}"),
            Self::Remote { message, .. } => write!(f, "远端故障: {message}"),
            Self::Conflict { message } => write!(f, "冲突: {message}"),
        }
    }
}

impl std::error::Error for InfraError {}

#[cfg(debug_assertions)]
pub(crate) static SQLITE_DIAGNOSTIC_PATH: std::sync::OnceLock<std::path::PathBuf> =
    std::sync::OnceLock::new();

impl From<sqlx::Error> for InfraError {
    #[track_caller]
    fn from(error: sqlx::Error) -> Self {
        #[cfg(debug_assertions)]
        {
            use std::io::Write;
            let code = error.as_database_error().and_then(|e| e.code());
            let line = format!(
                "SQLite error at {} (code {}): {}",
                std::panic::Location::caller(),
                code.as_deref().unwrap_or("unknown"),
                error,
            );
            eprintln!("{line}");
            if let Some(path) = SQLITE_DIAGNOSTIC_PATH.get() {
                if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    let _ = writeln!(log, "{line}");
                }
            }
        }
        #[cfg(not(debug_assertions))]
        let _ = error;
        Self::Storage {
            message: "本地数据库操作失败".into(),
        }
    }
}

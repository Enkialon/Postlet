//! 应用层错误：PortError → AppError 映射

use crate::domain::DomainError;

/// 应用边界出站错误；不属于 Domain，禁止泄漏 SQL / Token / SDK 错误详情
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortError {
    Storage { message: String },
    Auth { message: String },
    Network { message: String },
    Remote { message: String, retryable: bool },
    Conflict { message: String },
}

impl std::fmt::Display for PortError {
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

impl std::error::Error for PortError {}

impl From<sqlx::Error> for PortError {
    fn from(_: sqlx::Error) -> Self {
        Self::Storage { message: "本地数据库操作失败".into() }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        PortError::from(error).into()
    }
}

/// 可序列化到前端的应用错误码
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppErrorCode {
    NotFound,
    Validation,
    Conflict,
    Auth,
    Sync,
    Network,
    Internal,
}

/// 可序列化到前端的应用错误
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AppError {
    pub code: AppErrorCode,
    /// 稳定说明，可供 i18n 映射
    pub message: String,
    pub retryable: bool,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl AppError {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::NotFound,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::Validation,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::Conflict,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::Internal,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn not_implemented(feature: &str) -> Self {
        Self::internal(format!("尚未实现: {feature}"))
    }

    pub fn auth(message: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::Auth,
            message: message.into(),
            retryable: false,
        }
    }
}

impl From<DomainError> for AppError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::NotFound { entity, id } => Self::not_found(format!("{entity} 不存在: {id}")),
            DomainError::Conflict { message } => Self::conflict(message),
            DomainError::InvariantViolation { message } => Self::validation(message),
        }
    }
}

impl From<PortError> for AppError {
    fn from(err: PortError) -> Self {
        match err {
            PortError::Storage { message } => Self {
                code: AppErrorCode::Internal,
                message,
                retryable: false,
            },
            PortError::Auth { message } => Self {
                code: AppErrorCode::Auth,
                message,
                retryable: false,
            },
            PortError::Network { message } => Self {
                code: AppErrorCode::Network,
                message,
                retryable: true,
            },
            PortError::Remote { message, retryable } => Self {
                code: AppErrorCode::Sync,
                message,
                retryable,
            },
            PortError::Conflict { message } => Self::conflict(message),
        }
    }
}

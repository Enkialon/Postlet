//! 应用层错误：InfraError → AppError 映射

use crate::domain::DomainError;

use crate::infrastructure::error::InfraError;

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        InfraError::from(error).into()
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

    /// 同步类错误：可重试，用于夹级失败留痕
    pub fn sync(message: impl Into<String>) -> Self {
        Self {
            code: AppErrorCode::Sync,
            message: message.into(),
            retryable: true,
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

impl From<InfraError> for AppError {
    fn from(err: InfraError) -> Self {
        match err {
            InfraError::Storage { message } => Self {
                code: AppErrorCode::Internal,
                message,
                retryable: false,
            },
            InfraError::Auth { message } => Self {
                code: AppErrorCode::Auth,
                message,
                retryable: false,
            },
            InfraError::Network { message } => Self {
                code: AppErrorCode::Network,
                message,
                retryable: true,
            },
            InfraError::Remote { message, retryable } => Self {
                code: AppErrorCode::Sync,
                message,
                retryable,
            },
            InfraError::Conflict { message } => Self::conflict(message),
        }
    }
}

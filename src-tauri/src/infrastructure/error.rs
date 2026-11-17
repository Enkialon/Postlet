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

impl From<sqlx::Error> for InfraError {
    fn from(_: sqlx::Error) -> Self {
        Self::Storage {
            message: "本地数据库操作失败".into(),
        }
    }
}

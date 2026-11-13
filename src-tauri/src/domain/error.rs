/// 领域层错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    /// 实体不存在
    NotFound {
        entity: &'static str,
        id: String,
    },
    /// 冲突（合并环、重复根等）
    Conflict {
        message: String,
    },
    /// 不变量破坏
    InvariantViolation {
        message: String,
    },
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { entity, id } => write!(f, "{entity} 不存在: {id}"),
            Self::Conflict { message } => write!(f, "冲突: {message}"),
            Self::InvariantViolation { message } => write!(f, "不变量破坏: {message}"),
        }
    }
}

impl std::error::Error for DomainError {}

use crate::domain::DomainError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct CredentialRef(String);

impl CredentialRef {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.is_empty() || value.len() > 128 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')) {
            return Err(DomainError::InvariantViolation { message: "凭据引用无效".into() });
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

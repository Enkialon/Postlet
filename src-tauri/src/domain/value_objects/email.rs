//! 邮箱地址与 self 排除集合

use std::fmt;

/// 规范化邮箱：trim + lowercase
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct EmailAddress(String);

impl EmailAddress {
    /// 规范化后构造；空串返回 `None`
    pub fn parse(raw: impl AsRef<str>) -> Option<Self> {
        let normalized = raw.as_ref().trim().to_ascii_lowercase();
        if normalized.is_empty() {
            None
        } else {
            Some(Self(normalized))
        }
    }

    /// 假定输入已规范化
    pub fn new_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for EmailAddress {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EmailAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// self 排除集合 = {email} ∪ aliases
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelfAddressSet {
    pub addresses: Vec<EmailAddress>,
}

impl SelfAddressSet {
    /// 是否包含该地址
    pub fn contains(&self, address: &EmailAddress) -> bool {
        self.addresses.iter().any(|a| a == address)
    }
}

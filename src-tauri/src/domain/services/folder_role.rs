//! 文件夹角色（IMAP SPECIAL-USE 对齐）与默认展示范围。
//!
//! Graph 的 well-known 名称在 `infrastructure/graph` 内映射为本模块的角色字符串，
//! 领域与库表只认这里的角色，不出现 Graph 专名。

/// 统一文件夹角色（RFC 6154 SPECIAL-USE 对齐）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Junk,
    Archive,
    All,
    Custom,
}

impl FolderRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Sent => "sent",
            Self::Drafts => "drafts",
            Self::Trash => "trash",
            Self::Junk => "junk",
            Self::Archive => "archive",
            Self::All => "all",
            Self::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "inbox" => Self::Inbox,
            "sent" => Self::Sent,
            "drafts" => Self::Drafts,
            "trash" => Self::Trash,
            "junk" => Self::Junk,
            "archive" => Self::Archive,
            "all" => Self::All,
            "custom" => Self::Custom,
            _ => return None,
        })
    }

    /// 默认普通会话/未读范围是否排除该角色
    pub fn excluded_from_default_view(self) -> bool {
        matches!(self, Self::Trash | Self::Junk | Self::Drafts | Self::All)
    }

    /// 是否属于垃圾夹：正文默认不渲染
    pub fn is_junk(self) -> bool {
        self == Self::Junk
    }
}

/// 角色来源：决定是否可被自动刷新覆盖
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleSource {
    /// 服务器 SPECIAL-USE / well-known
    Server,
    /// 用户手动指定
    Manual,
    /// 名称启发式（仅作候选，可被服务器结果覆盖）
    Guess,
    Unknown,
}

impl RoleSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Manual => "manual",
            Self::Guess => "guess",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "server" => Self::Server,
            "manual" => Self::Manual,
            "guess" => Self::Guess,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }
}

/// IMPAP SPECIAL-USE 属性名（`\Inbox` / `\Junk` …）→ 角色
pub fn role_from_special_use(attributes: &[String]) -> Option<FolderRole> {
    for attr in attributes {
        let name = attr.trim_start_matches('\\').to_ascii_lowercase();
        let role = match name.as_str() {
            "inbox" => FolderRole::Inbox,
            "sent" | "sentmail" | "sentitems" => FolderRole::Sent,
            "drafts" => FolderRole::Drafts,
            "trash" | "deleted" | "deleteditems" => FolderRole::Trash,
            "junk" | "spam" => FolderRole::Junk,
            "archive" | "allmail" => FolderRole::Archive,
            "all" => FolderRole::All,
            _ => continue,
        };
        return Some(role);
    }
    None
}

/// 名称启发式：只作候选，命中返回 Guess 来源
pub fn role_from_display_name(name: &str) -> Option<FolderRole> {
    let lower = name.trim().to_ascii_lowercase();
    let raw = name.trim();
    let candidate = if lower == "inbox" || raw == "收件箱" {
        FolderRole::Inbox
    } else if lower.contains("sent") || raw.contains("已发送") {
        FolderRole::Sent
    } else if lower.contains("draft") || raw.contains("草稿") {
        FolderRole::Drafts
    } else if lower.contains("trash")
        || lower.contains("deleted")
        || raw.contains("已删除")
        || raw.contains("废纸篓")
    {
        FolderRole::Trash
    } else if lower.contains("junk")
        || lower.contains("spam")
        || raw.contains("垃圾邮件")
        || raw.contains("垃圾箱")
    {
        FolderRole::Junk
    } else if lower.contains("archive") || raw.contains("归档") {
        FolderRole::Archive
    } else if lower == "all" || lower == "all mail" || raw.contains("所有邮件") {
        FolderRole::All
    } else {
        return None;
    };
    Some(candidate)
}

/// 合并角色：服务器/手动优先；名称猜测只填补缺失
pub fn resolve_role(
    server_role: Option<FolderRole>,
    manual_role: Option<FolderRole>,
    display_name: &str,
) -> (FolderRole, RoleSource) {
    if let Some(role) = manual_role {
        return (role, RoleSource::Manual);
    }
    if let Some(role) = server_role {
        return (role, RoleSource::Server);
    }
    if let Some(role) = role_from_display_name(display_name) {
        return (role, RoleSource::Guess);
    }
    (FolderRole::Custom, RoleSource::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_use_beats_name_guess() {
        let attrs = vec!["\\Junk".to_string()];
        assert_eq!(role_from_special_use(&attrs), Some(FolderRole::Junk));
        assert_eq!(
            resolve_role(
                role_from_special_use(&attrs),
                None,
                "归档"
            ),
            (FolderRole::Junk, RoleSource::Server)
        );
    }

    #[test]
    fn manual_role_wins_over_server() {
        assert_eq!(
            resolve_role(Some(FolderRole::Inbox), Some(FolderRole::Junk), "INBOX"),
            (FolderRole::Junk, RoleSource::Manual)
        );
    }

    #[test]
    fn chinese_names_are_only_guesses() {
        assert_eq!(
            resolve_role(None, None, "垃圾邮件"),
            (FolderRole::Junk, RoleSource::Guess)
        );
        assert_eq!(
            resolve_role(None, None, "自定义夹"),
            (FolderRole::Custom, RoleSource::Unknown)
        );
    }

    #[test]
    fn default_view_excludes_junk_trash_drafts_all() {
        for role in [FolderRole::Junk, FolderRole::Trash, FolderRole::Drafts, FolderRole::All] {
            assert!(role.excluded_from_default_view(), "{role:?}");
        }
        for role in [FolderRole::Inbox, FolderRole::Sent, FolderRole::Archive, FolderRole::Custom] {
            assert!(!role.excluded_from_default_view(), "{role:?}");
        }
    }
}

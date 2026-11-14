//! IMAP 文件夹游标：复用 mail_folder_sync_state.delta_link / next_link。
//!
//! - delta_link: `imap-uidvalidity:{n}`
//! - next_link:  `imap-uid:{last_uid}`

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImapFolderCursor {
    pub uidvalidity: u32,
    pub last_uid: u32,
}

pub fn encode_uidvalidity(uidvalidity: u32) -> String {
    format!("imap-uidvalidity:{uidvalidity}")
}

pub fn encode_last_uid(last_uid: u32) -> String {
    format!("imap-uid:{last_uid}")
}

pub fn parse_uidvalidity(delta_link: Option<&str>) -> Option<u32> {
    let raw = delta_link?;
    let rest = raw.strip_prefix("imap-uidvalidity:")?;
    rest.parse().ok()
}

pub fn parse_last_uid(next_link: Option<&str>) -> Option<u32> {
    let raw = next_link?;
    let rest = raw.strip_prefix("imap-uid:")?;
    rest.parse().ok()
}

pub fn parse_cursor(
    delta_link: Option<&str>,
    next_link: Option<&str>,
) -> Option<ImapFolderCursor> {
    Some(ImapFolderCursor {
        uidvalidity: parse_uidvalidity(delta_link)?,
        last_uid: parse_last_uid(next_link).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_cursor() {
        let uv = encode_uidvalidity(42);
        let uid = encode_last_uid(100);
        let c = parse_cursor(Some(&uv), Some(&uid)).unwrap();
        assert_eq!(c.uidvalidity, 42);
        assert_eq!(c.last_uid, 100);
    }

    #[test]
    fn rejects_foreign_delta() {
        assert!(parse_uidvalidity(Some("https://graph.microsoft.com/...")).is_none());
    }
}

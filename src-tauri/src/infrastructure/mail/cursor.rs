//! IMAP 文件夹游标：复用 mail_folder_sync_state.delta_link / next_link。
//!
//! - delta_link: `imap-uidvalidity:{n}`
//! - next_link:  `imap-uid:{high}` 或 `imap-uid:{high}:{low}`
//!   - high：当前快照中已下载的最大 UID（仅进度信息，不用于排除下载候选）
//!   - low：最新未下载 UID 的上界；1 表示当前快照全部完成，0 表示尚未核对

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImapFolderCursor {
    pub uidvalidity: u32,
    /// 已下载的最大 UID；每轮按实际差集补齐，不根据它跳过历史。
    pub high_uid: u32,
    /// 最新未下载 UID 的上界；1 = 完成，0 = 尚未核对。
    pub low_uid: u32,
}

impl ImapFolderCursor {
    /// 回填上界：low 未写时保守视为 high（会扫 1..high-1，已知 UID 会跳过）
    pub fn backfill_low(&self) -> u32 {
        if self.low_uid == 0 {
            self.high_uid
        } else {
            self.low_uid
        }
    }
}

pub fn encode_uidvalidity(uidvalidity: u32) -> String {
    format!("imap-uidvalidity:{uidvalidity}")
}

/// 仅 high（旧格式）
pub fn encode_last_uid(high_uid: u32) -> String {
    format!("imap-uid:{high_uid}")
}

/// high + low
pub fn encode_uid_cursor(high_uid: u32, low_uid: u32) -> String {
    if low_uid == 0 {
        encode_last_uid(high_uid)
    } else {
        format!("imap-uid:{high_uid}:{low_uid}")
    }
}

pub fn parse_uidvalidity(delta_link: Option<&str>) -> Option<u32> {
    let raw = delta_link?;
    let rest = raw.strip_prefix("imap-uidvalidity:")?;
    rest.parse().ok()
}

pub fn parse_last_uid(next_link: Option<&str>) -> Option<u32> {
    parse_uid_pair(next_link).map(|(high, _)| high)
}

fn parse_uid_pair(next_link: Option<&str>) -> Option<(u32, u32)> {
    let raw = next_link?;
    let rest = raw.strip_prefix("imap-uid:")?;
    let mut parts = rest.split(':');
    let high: u32 = parts.next()?.parse().ok()?;
    let low = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Some((high, low))
}

pub fn parse_cursor(
    delta_link: Option<&str>,
    next_link: Option<&str>,
) -> Option<ImapFolderCursor> {
    let (high_uid, low_uid) = parse_uid_pair(next_link).unwrap_or((0, 0));
    Some(ImapFolderCursor {
        uidvalidity: parse_uidvalidity(delta_link)?,
        high_uid,
        low_uid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_high_only() {
        let uv = encode_uidvalidity(42);
        let uid = encode_last_uid(100);
        let c = parse_cursor(Some(&uv), Some(&uid)).unwrap();
        assert_eq!(c.uidvalidity, 42);
        assert_eq!(c.high_uid, 100);
        assert_eq!(c.low_uid, 0);
        assert_eq!(c.backfill_low(), 100);
    }

    #[test]
    fn roundtrip_high_low() {
        let uv = encode_uidvalidity(1);
        let uid = encode_uid_cursor(200, 50);
        let c = parse_cursor(Some(&uv), Some(&uid)).unwrap();
        assert_eq!(c.high_uid, 200);
        assert_eq!(c.low_uid, 50);
        assert_eq!(c.backfill_low(), 50);
    }

    #[test]
    fn rejects_foreign_delta() {
        assert!(parse_uidvalidity(Some("https://graph.microsoft.com/...")).is_none());
    }
}

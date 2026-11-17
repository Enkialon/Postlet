//! 按服务器实际 UID 集合补齐本地缺失邮件；UID 数字不要求连续。

use std::collections::HashSet;

/// 最新优先，但每轮都从实际缺失集合选取，不会因游标越过而漏掉历史或中间缺口。
pub fn missing_imap_page(remote: &HashSet<u32>, downloaded: &HashSet<u32>, limit: u32) -> Vec<u32> {
    let mut missing: Vec<_> = remote.difference(downloaded).copied().collect();
    missing.sort_unstable_by(|a, b| b.cmp(a));
    if limit > 0 {
        missing.truncate(limit as usize);
    }
    missing.reverse();
    missing
}

/// 最新未完成 UID 的上界；1 表示当前服务器快照已全部下载。
/// 即使本地已有 UID=1，只要中间仍有缺口，就不能宣布回填完成。
pub fn imap_pending_boundary(remote: &HashSet<u32>, downloaded: &HashSet<u32>) -> u32 {
    remote
        .difference(downloaded)
        .max()
        .map(|uid| uid.saturating_add(1))
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_history_crosses_uid_gaps_and_resumes_from_saved_messages() {
        let remote = HashSet::from([1, 17, 700, 998, 1000]);
        let mut saved = HashSet::from([1000]);
        assert_eq!(missing_imap_page(&remote, &saved, 2), vec![700, 998]);
        // 模拟提交一批、进程退出后，仅凭已落库 UID 恢复。
        saved.extend([700, 998]);
        assert_eq!(missing_imap_page(&remote, &saved, 2), vec![1, 17]);
        saved.extend([1, 17]);
        assert_eq!(imap_pending_boundary(&remote, &saved), 1);
    }

    #[test]
    fn more_than_one_batch_of_new_mail_does_not_skip_the_middle() {
        let remote: HashSet<_> = (1..=550).collect();
        let mut saved: HashSet<_> = (1..=100).collect();
        for _ in 0..3 {
            let page = missing_imap_page(&remote, &saved, 200);
            assert!(page.len() <= 200);
            saved.extend(page);
        }
        assert_eq!(saved, remote);
    }

    #[test]
    fn failed_mail_remains_pending_without_blocking_older_mail() {
        let remote = HashSet::from([1, 10, 20, 30]);
        let mut saved = HashSet::from([1, 30]);
        assert_eq!(missing_imap_page(&remote, &saved, 2), vec![10, 20]);
        saved.insert(10); // UID 20 没有正文，不能视为成功。
        assert_eq!(imap_pending_boundary(&remote, &saved), 21);
        assert_eq!(missing_imap_page(&remote, &saved, 2), vec![20]);
        saved.insert(20);
        assert_eq!(imap_pending_boundary(&remote, &saved), 1);
    }

    #[test]
    fn deleted_uids_and_empty_mailboxes_do_not_leave_pending_history() {
        assert_eq!(
            imap_pending_boundary(&HashSet::new(), &HashSet::from([42])),
            1
        );
        let remote = HashSet::from([9]);
        assert!(missing_imap_page(&remote, &HashSet::from([9, 42]), 200).is_empty());
        assert_eq!(imap_pending_boundary(&remote, &HashSet::from([9, 42])), 1);
    }

    #[test]
    fn uid_upper_bound_does_not_overflow() {
        let remote = HashSet::from([u32::MAX]);
        assert_eq!(
            missing_imap_page(&remote, &HashSet::new(), 200),
            vec![u32::MAX]
        );
        assert_eq!(imap_pending_boundary(&remote, &HashSet::new()), u32::MAX);
    }
}

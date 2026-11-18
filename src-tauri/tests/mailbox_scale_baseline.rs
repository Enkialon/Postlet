//! Task 17：代表性邮箱规模的差集基线（无网络）。
//!
//! 只测量「UID 全集差集」这一正确性基线在常见规模下的耗时，
//! 不在未发现瓶颈时引入高水位优化。

use postlet_lib::domain::services::{imap_pending_boundary, missing_imap_page};
use std::collections::HashSet;
use std::time::Instant;

fn run(label: &str, remote_count: u32, downloaded_ratio: u32, page: u32) {
    let remote: HashSet<u32> = (1..=remote_count).collect();
    let mut downloaded: HashSet<u32> = remote
        .iter()
        .copied()
        .filter(|uid| uid % downloaded_ratio != 0)
        .collect();
    let started = Instant::now();
    let mut rounds = 0u32;
    loop {
        let page_uids = missing_imap_page(&remote, &downloaded, page);
        if page_uids.is_empty() {
            break;
        }
        rounds += 1;
        downloaded.extend(page_uids);
        if rounds > remote_count + 8 {
            panic!("{label}: pagination did not converge");
        }
    }
    let elapsed = started.elapsed();
    assert_eq!(downloaded, remote, "{label}: all UIDs must be covered");
    assert_eq!(imap_pending_boundary(&remote, &downloaded), 1);
    eprintln!(
        "{label}: remote={} page={} rounds={} elapsed={}ms",
        remote_count,
        page,
        rounds,
        elapsed.as_millis()
    );
    // 内存差集必须在常见规模下保持数量级内的开销
    assert!(elapsed.as_secs() < 10, "{label}: set difference too slow");
}

#[test]
fn mailbox_scale_baseline() {
    run("small-2k", 2_000, 3, 200);
    run("medium-20k", 20_000, 4, 200);
    run("large-100k", 100_000, 5, 200);
}

#[test]
fn uid_gaps_and_empty_ranges_stay_stable() {
    let remote: HashSet<u32> = [1, 2, 10_000, 10_001].into_iter().collect();
    let downloaded = HashSet::new();
    assert_eq!(missing_imap_page(&remote, &downloaded, 2).len(), 2);
    // 最大缺失 UID + 1：表示「最新未完成上界」
    assert_eq!(imap_pending_boundary(&remote, &HashSet::new()), 10_002);
    assert!(missing_imap_page(&remote, &remote, 200).is_empty());
}

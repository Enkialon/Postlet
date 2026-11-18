# IMAP / SMTP 修复验收记录（Task 17）

- 执行日期：2026-11-17
- 平台：Windows（WebView2 / MSVC），Node 22 + pnpm，Rust stable
- 结论范围：只有下列命令实际执行并通过；未列出的平台与真实邮箱/OAuth 场景未验证。

## 命令与结果

| 项目 | 命令 | 结果 |
|---|---|---|
| Rust 全部测试 | `cd src-tauri && cargo test --offline --tests` | 通过：lib 72 + attachment_storage 4 + imap_history_sync 1 + mail_account_configuration 1(+1 ignored) + mail_protocol_routing 2 + repository_transactions 6 + end_to_end_acceptance 3 + mailbox_scale_baseline 2 |
| 前端类型检查 | `npx vue-tsc --noEmit` | 通过 |
| 会话列表顺序自检 | `npx tsx src/features/conversations/conversationListOrder.selftest.ts` | 通过 |
| 邮件内容安全（dev + production CSP） | `npx playwright test tests/mail-content` | 8 项通过 |
| 真实 WebView2 隔离（生产 exe） | `pnpm test:mail-content:tauri -- --mode=release` | 报告 `tests/mail-content/last-tauri-webview-report-release.json`：`allowsScripts=false`、`ipcProbeResult=no-ipc-side-effect`、授权前远程请求为 0 |

## 邮箱规模基线（UID 全集差集）

`cargo test --test mailbox_scale_baseline -- --nocapture`

| 规模 | 每页 | 轮次 | 耗时 |
|---|---|---|---|
| 2,000 | 200 | 4 | 2 ms |
| 20,000 | 200 | 25 | 91 ms |
| 100,000 | 200 | 100 | 1,873 ms |

结论：差集 + 分页在 100k UID 量级仍是毫秒至低秒级；未发现需要引入高水位算法的瓶颈，保持 `UID SEARCH ALL` 差集基线。

## 按问题编号的覆盖

- I-01 / I-10（删除）：默认移入 Trash、永久删除保留 UIDPLUS + 单封 UID EXPUNGE；`end_to_end_acceptance::default_delete_needs_a_confirmed_trash_target`。
- I-06 / I-07 / I-08（发送）：发送队列状态机、不可变快照、结果分类；`send_tasks`、`archive_tasks`、`outbound_content` 单测与 `attachment_storage` 集成测试。
- I-12（MIME）：附件/正文/日期/地址组；`imap::parse::mime_tests`。
- I-13（同步批次）：逐夹提交 + 失败候选退避；`download_candidates` 单测。
- I-15（连接预算）：登录使用独立阶段 deadline，登录成功后由 `MailOperation` 单独约束；`imap::connection` 既有测试。
- I-17 / I-18（认证与结果）：收发认证拆分、SMTP/Graph 分类；`smtp::send` 与 `graph::mail` 单测。
- I-19（HTML 隔离）：Playwright + WebView2 报告（含同源残留说明，见 Task 16 卡片）。

## 未执行 / 未验证（如实标注）

- 真实 Graph OAuth 登录与真实 IMAP/SMTP 服务器收发（无凭据与隔离测试邮箱）。
- I-09 通用 OAuth2/XOAUTH2 完整生命周期：仍属暂缓项，不在本轮范围。
- macOS WKWebView、Linux WebKitGTK 的隔离验证；打包安装器 UI 冒烟。
- 跨进程执行互斥（单实例假设）。

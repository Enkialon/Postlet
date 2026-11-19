# V1 发信状态与 Sent 归档范围建议

状态：待确认。本文件只记录范围建议和 P1 处置，不修改代码，也不替代现有 Task 14 设计。

## 业务范围

- **V1 保留可靠发信，不承诺客户端补写服务端 Sent。** SMTP 提交不保证服务器保存副本。服务商代存或 Graph 保存的副本继续走普通邮件同步；不代存时，不保证在网页、手机等其他客户端的「已发送」里看到这封邮件。若跨端 Sent 是明确需求，再启用客户端补存方案。
- **发送状态只依据提交结果。** 排队/发送中、服务端已接受、明确失败、结果未知。已接受不等于已送达；没有 Sent 副本不等于发送失败。最终确认丢失时记录「结果未知」，不得自动重发；仅在有证据表明未被接受且错误可重试时才重试。
- 保留持久化发送队列、发送内容快照和无远端位置的发送状态展示。不要用 Message-ID 猜测发送成功，也不要要求先出现 Sent 副本才显示服务端已接受。

## P1 不随归档一起消失

以 [11 月 19 日复审](./2026-11-19-review-fix-plan.md) 的编号为准。该复审以 `a92edd3` 为基线；当前 `66dfbec` 已包含后续修复提交，以下区分代码变更与验收结论。

| 编号 | 风险 | 当前处置 |
|---|---|---|
| F01 | 切换或退出账号时，后台任务仍可能发送旧账号邮件；取消也可能发生在服务端已接受之后。 | **发信侧仍须验收。** `55bcdfd` 已接入取消链路。验证取消前未提交就不发、确认丢失后不重发、已有成功确认不被取消覆盖、账号清理等待结果落库。仅凭修复提交不能判定验收通过。 |
| F02 | IMAP APPEND 已写入但确认丢失，误重试会产生重复 Sent 副本。 | `55bcdfd` 已处理相关结果语义。若确认 V1 延期归档，应停用客户端 APPEND 路径；届时该风险不再是 V1 发信阻塞项。 |
| F03 | 仅凭 Message-ID 确认 Sent 副本，可能确认错邮件。 | `912ba15` 已移除这条猜测确认路径。即使延期归档，也不应把它迁到发送状态判断。 |

另外，前序 [11 月 18 日审查](./2026-11-18-code-review.md) 的 R01（Graph 确认丢失后误重发）同样与 Sent 无关。SMTP 和 Graph 都应验证：成功响应可持久化；请求已提交但未获得确认时，不自动重发。本文未执行动态验收。

## 确认范围后再改代码

若确认延期，按以下顺序实施；更新 [Task 14](./tasks/imap-smtp/14-sent-archive.md) 标记延期。

1. **切断归档入队。** 在 `src-tauri/src/application/use_cases/outbound_use_case.rs` 的 `complete_accepted_attempt` 去掉 `enqueue_archive`，删除该私有函数。保留发送任务的条件更新、账号版本更新及其同一事务；`send_worker.rs` 仍在提交后派发变更。数据库写入失败不得重新执行网络发送。
2. **删掉归档专用后端。** 从 `src-tauri/src/lib.rs` 移除归档 worker 启动，从 `runtime/mod.rs` 和 repository `mod.rs` 移除对应模块，再删除 `archive_worker.rs`、`repository/archive_tasks.rs`。清理 `account_use_case.rs`、`repository/accounts.rs` 中仅用于归档的账号清理逻辑。检查 `append_message_async` 及其辅助函数的剩余调用：只删归档独占代码，不删共享 IMAP 连接和取消能力。
3. **移除归档策略设置与存储。** 从 `SettingsPage.vue`、`useSettings.ts`、`accountPort.ts`、Tauri/Memory adapter 到 Rust command、IPC 权限、DTO、账号实体及仓储映射，删除归档策略读写路径。直接修改 `src-tauri/migrations/001_init.sql`，移除 `archive_tasks` 和 `sent_archive_policy` 等归档专用结构；V1 不保留兼容垫片，验证使用新建开发库。保留 Sent 文件夹角色及同步，不改 Graph 保存副本的行为。
4. **单独收尾 F01。** 沿 `runtime/send_worker.rs` → `outbound_use_case::execute_claimed_attempt` → SMTP/Graph 核对使用同一个账号 `MailOperation`。在进入远端提交前检查取消；提交开始后按协议证据分类：确认接受则保留成功，能证明未接受才允许按策略重试，否则记结果未知。持有账号 permit 到网络操作退出且结果事务收尾，不能因取消提前放行账号清理。当前已有接线，先补测试，只有测试暴露缺口才修改实现。

### 验收

- 通过真实 worker 调用链验证取消发生在提交前、提交后无确认、收到成功后但落库前这三种场景；分别断言不发出提交、未知且后续 tick 不重发、成功且账号清理等待结果落库。覆盖 SMTP 和 Graph，不以单测错误分类函数代替。
- SMTP 不代存副本时仍能完成发送并显示已接受，且不发 IMAP APPEND；服务端已有 Sent 副本时仍能正常同步展示。不得依赖 Message-ID 自动确认发送或把普通同步副本与发送状态强行合并。
- 删除归档专用测试，不删除发送和普通 Sent 同步断言；跑相关 Rust 回归测试和前端类型检查，并确认不再存在归档 IPC、设置入口和后台任务。

# Task 13：按文件夹提交并隔离坏邮件重试

- 状态：已完成
- 实现备注：新增 `mail_download_candidates` 与仓储（record_discovered/list_due/mark_downloaded/record_failure/invalidate_generation），失败候选退避且不阻塞更旧候选；IMAP 单夹提交拆为 `apply_imap_folder_batch`，单夹失败不丢弃已提交批次（仅全失败才返回 Err）；失败 UID 与成功内容在同一事务落库。
- 对应问题：I-13、A-03
- 前置任务：05、12

## 修改位置

- `src-tauri/src/infrastructure/imap/session_ops.rs、src-tauri/src/infrastructure/imap/sync.rs、src-tauri/src/application/use_cases/sync_use_case.rs`：移除先收齐所有文件夹才返回的控制流；每次只同步并提交一个文件夹的一批，再交付本批 ChangeSet。
- `src-tauri/migrations/001_init.sql、src-tauri/src/infrastructure/repository/`：持久化发现候选、成功下载及失败重试事实；记录固定位置、尝试次数、下次时间和可见错误。
- `src-tauri/src/infrastructure/graph/mail.rs、src-tauri/src/runtime/mail_sync.rs、src/features/sync/useSyncStatus.ts`：Graph 稀疏消息补取也逐项隔离；runtime 继续其他夹/批并汇总失败，前端可查看失败进度。

## 实施步骤

1. 保留 UID SEARCH ALL + 本地差集作为正确性基线，UID 不要求连续；每批提交后版本和变更同时交付。
2. 单 UID 解析或内容失败记录退避，后续候选和更旧历史继续；失败项不能计为已成功下载。
3. Graph 推进游标前，必须在同一事务保存已完成变化和可恢复的待补取候选；404 只处理原位置移出。
4. 发现进度、已持久化内容和失败候选分开保存；不要因为性能猜测提前引入高水位算法。

## 验收

- known/已下载集合不得包含 location_status != present 的消息；同代同绑定下服务端仍有的 UID 必须重新进入下载候选，并经 Upsert 恢复 Missing 位置。仅过滤差集输入，不复活 invalidated 的旧代或退役位置。
- 单 UID/整页失败只退避对应候选；冷却中的 UID 从本轮候选跳过并继续更旧候选，不得因最新失败 UID 反复占页而永久阻塞历史补齐或 IDLE。仅剩冷却候选时允许进入 IDLE/有限等待，并在到期时唤醒重试。

- 两夹成功、后一夹失败时前两夹数据和事件保留；整页坏信不会阻塞更旧邮件。
- 重启、UID 缺口、下载失败后恢复及 Graph 稀疏项失败均不丢候选或错误推进游标。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。

## 本任务独占的职责与接口

- Task 04 只提供用例返回已提交结果、runtime 分发的边界。本任务独占逐夹/逐批控制流和“前两批成功、第三批失败”的协议集成验收。
- application/use_cases/sync_use_case.rs 增加 sync_folder_batch：输入账号、本地 FolderId、预期绑定/定位修订及操作控制，单次只处理一夹的一批；成功返回本批已提交变更与进度，失败不伪造 ChangeSet。
- runtime/mail_sync.rs 逐批调用并立即交付成功结果，再进入下一批；按夹汇总错误，不能把前面已提交的摘要转换成整体 Err。连接复用不能重新引入全夹事务或延迟所有批次交付。
- 新增 infrastructure/repository/download_candidates.rs；mail_download_candidates 最低字段：account_id、folder_id、remote_key、binding_revision、generation、state、attempts、next_attempt_at、last_error。以位置唯一，remote_key/generation 对 application 保持不透明。
- 仓储函数：record_discovered、list_due、mark_downloaded、record_failure、invalidate_generation。发现候选和发现游标同事务；消息/附件引用、下载成功、投影和账号版本在成功落库事务内一起提交。
- Graph 页内每项均已应用或持久化为可重取候选后，才可推进游标。Task 05 的版本测试可使用独立数据库事务，本任务补真实协议批次场景。


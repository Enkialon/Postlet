# Task 10：恢复已读与删除意图并阻止旧结果覆盖

- 状态：已完成
- 实现备注：mutation 增加 `claim_token/claimed_version/lease_until/execution_phase/last_error_code`；`put_read/put_delete` 的 INSERT…SELECT 影响行数为 0 时返回 Conflict（同事务回滚乐观更新）。`claim_due` 租约内不可二次认领；`record_attempt_result`/`acknowledge` 校验版本与认领标识，旧结果不能确认新意图；启动时 `recover_interrupted` 清理认领等待重新核对。调度器对两种协议都周期扫描到期任务。
- 对应问题：I-06、I-10、A-04
- 前置任务：05、08、09
- 对 Task 08 的依赖仅为旧消息状态拆分落地；不必等待发送 worker 全功能完成。

## 修改位置

- `src-tauri/src/domain/entities/message_mutation.rs、src-tauri/src/infrastructure/repository/mutations.rs、src-tauri/migrations/001_init.sql`：在现有固定目标、位置/绑定修订和版本上补认领、执行阶段、lease、重试和可见错误；版本确认后也不复用。
- `src-tauri/src/application/use_cases/message_use_case.rs、src-tauri/src/runtime/mail_sync_scheduler.rs`：两个协议都周期扫描到期任务；发送前校验登录、最新版本、固定目标和角色，按消息串行。
- `src-tauri/src/application/use_cases/sync_use_case.rs、src-tauri/src/infrastructure/imap/sync.rs、src/features/conversations/`：为远端观测增加防晚到的本地序号或等效修订；暴露待重试、失败和停止原因。

## 实施步骤

1. 同一 Message 的远端写串行，旧请求完成只能确认对应版本；期间产生反向意图时继续推送新目标值。
2. 旧位置消失、UIDVALIDITY 或绑定修订失效时停止原任务，不通过内容、Message-ID 或同名目录重新定位。
3. 远端成功但本地确认失败时，进程存活只重试本地确认；重启后已读 set 可在目标仍有效时幂等重放。
4. 同步读取在已确认写操作之前发起的结果不得覆盖较新确认值；过期读取丢弃并重新核对。

## 验收

- put_read/put_delete 的 INSERT…SELECT 影响行数为 0 时必须返回失败，并回滚同事务中的本地乐观更新；检查 rows_affected 即可，禁止静默成功，不新增框架。

- IMAP 离线改已读后无新操作也能恢复；true→false 的旧响应晚到最终收敛 false。
- 旧代 UID、位置移动、角色变化和 SQL 确认失败均不改错目标；Graph 与 IMAP 都覆盖。
- 旧连接阻塞超过 lease_until 时，再触发即时和周期 worker，不能出现第二个同目标网络写；暂停账号需等待旧尝试实际退出。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。

## 认领与旧连接的排他规则

1. 新增 runtime/mutation_worker.rs，入口复用 Task 04 的账号 permit 和操作登记；先取得 permit，再查询/认领到期任务。按账号串行是 V1 可接受的简单实现，无需再建独立的每消息锁框架。
2. 在现有 mutation 行补充 claim_token、claimed_version、lease_until、execution_phase、attempts、last_error_code。保存正在执行尝试的固定目标快照；新的业务意图只递增意图版本，不能覆盖旧尝试的目标、认领标识或执行阶段。
3. repository/mutations.rs 增加 claim_due、begin_attempt、record_attempt_result、recover_interrupted；SQL 条件更新同时检查账号、消息、意图/尝试版本和 claim_token。旧结果不能 acknowledge 新意图或清理新错误。
4. lease 到期仅表示需要恢复核对；仍被实际 worker 持有的 permit 不释放，新 worker 不得认领并行执行。先取消旧操作并 await 实际退出，才可恢复；续租也不能代替实际退出证明。
5. 旧请求已发送但新意图到达时，等待旧请求完成，记录对应尝试结果，再推最新目标值。旧读取结果依据本地观测序号拒绝覆盖新确认；后续外部客户端的新修改仍正常同步。
6. 停止操作后的本地确认/错误持久化完成后才释放 permit。已读可按固定有效位置幂等恢复；永久删除核对源位置，移动/复制的未知结果留给 Task 11 核对，不能按已读规则重放。

Task 08 负责移除旧 MessageSyncStatus 混用；本任务只在已明确的定位、意图和任务状态上补恢复，不重复重构发送状态。


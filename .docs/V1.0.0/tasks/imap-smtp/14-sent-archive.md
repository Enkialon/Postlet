# Task 14：实现已发送副本归档和展示

- 状态：**退出 V1 范围**（按 [V1 设计修复：发信记录留在会话](../../v1-conversation-send-no-sent-design.md)）
- 退出说明：V1 不提供独立「已发送」目录，也不维护 Sent 副本；服务端受理即「已发送」，会话内的本地发信记录就是发信历史。客户端归档策略、归档任务/worker、APPEND 专用实现及设置入口已随该设计移除；不承诺下版恢复，也不以本任务为「已实现」。普通邮件同步与 Sent 角色识别不变。
- 历史状态：本任务曾在旧方案下实现，现已被上述设计取代。
- 对应问题：I-11、A-06
- 前置任务：06、08、09

## 修改位置

- `src-tauri/migrations/001_init.sql、src-tauri/src/domain/entities/、src-tauri/src/infrastructure/repository/`：保存账号 Sent 策略及独立归档任务：服务器保存或客户端 APPEND，记录尝试、结果和不确定状态。
- `src-tauri/src/application/use_cases/outbound_use_case.rs、src-tauri/src/infrastructure/{imap,graph}/`：服务器已接受后按策略归档；客户端 APPEND 复用任务 06 的 MIME，协议结果在适配器内转换为确定目标位置。
- `src-tauri/src/infrastructure/repository/{messages,sessions}.rs、src/features/conversations/`：无位置发送记录在发送状态区显示；普通会话以实际 Sent 位置消息为准。

## 实施步骤

1. 归档失败只重试归档，不重新 SMTP/Graph 发送；服务器自动保存模式不能因副本延迟自动 APPEND。
2. 有确定目标位置且本地发送记录尚无位置时可首次绑定；若目标已存在，任务引用已有实体，保留原记录，不合并。
3. APPEND 最终响应或映射未知时先核对 Sent；无法确定则保留待核对，不盲目再次 APPEND，也不按 Message-ID/正文猜测合并。
4. 归档 worker 首次启用即接入 Task 04 的受控入口；退出时停止派发并等待原尝试收尾。APPEND 已开始且结果未知时先记录 uncertain，再释放 permit，不能延期到 Task 15。

## 验收

- 服务器保存和客户端 APPEND 两种策略；APPEND 拒绝、断线、本地回滚及自动副本延迟。
- Sent 同位置幂等、不同位置独立；A 给已登录 B 发信时 A/Sent 与 B/INBOX 不联动。
- APPEND 阻塞期间退出账号，旧尝试结束或记录不确定结果后才执行账号变更；lease 到期不能并行执行第二次 APPEND。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。

## 表、模块和状态归属

- 新增 domain/entities/archive_task.rs、infrastructure/repository/archive_tasks.rs、application/use_cases/archive_use_case.rs、runtime/archive_worker.rs。
- 账号策略为 server_saved 或 client_append；未确认策略时为 needs_configuration，不同时执行两种策略。任务阶段至少包含 waiting_server_copy、append_pending、appending、confirmed、retryable、uncertain、needs_configuration。
- archive_tasks 最低字段：task_id、account_id、send_task_id、snapshot_id、state、target_folder_id、target_binding_revision、attempt_id、claim_token、lease_until、next_attempt_at、last_error、confirmed_message_id。目标是同账号已确认的 Sent 文件夹。
- 函数：enqueue_after_acceptance、claim_due、record_append_result、confirm_location、record_failure、recover_interrupted。发送接受确认与创建归档任务同事务；归档进度、确定位置入库/首次绑定、Sent Message 关联及账号版本同事务。
- APPENDUID 仅在适配器内转换为确定位置；该位置已存在时只设置 confirmed_message_id 指向已有实体，原无位置记录留在发送状态区。
- 不以 Message.sync_status 表示归档；Task 08 已删除的状态混用不得恢复。归档失败、位置未同步和发送接受互不替代。


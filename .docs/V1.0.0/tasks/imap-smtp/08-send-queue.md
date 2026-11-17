# Task 08：持久化发送队列及前端状态

- 状态：进行中（发送队列已落地；消息状态解耦待完成）
- 已完成：`send_tasks` 表与仓储、`SendState` 状态机、`runtime/send_worker`（认领→尝试→落库、启动恢复 sending→uncertain）、compose 同事务入队并返回 `queued`、SMTP 按快照字节 `send_raw`、Graph 按快照 envelope 发送、IPC `get_send_state`、前端 `ComposeSendResult`/`SendState`。
- 待完成：按任务卡拆除 `MessageSyncStatus` 的发送/删除混用（消息可见性 SQL、乐观已读/删除意图与前端过滤）。
- 对应问题：I-08、I-07、A-06
- 前置任务：05、06、07

## 修改位置

- `src-tauri/migrations/001_init.sql、src-tauri/src/domain/entities/、src-tauri/src/infrastructure/repository/`：新增发送任务及仓储：不可变内容引用、阶段、尝试版本、lease、错误和重试时间。
- `src-tauri/src/application/use_cases/{compose,outbound}_use_case.rs、src-tauri/src/runtime/`：消息、附件引用、发送任务及账号版本同事务入队；worker 条件认领并在进程重启后恢复。
- `src/ports/composePort.ts、src/domain/{dto,entities}.ts、src/features/compose/、src/features/conversations/`：返回并展示排队、发送中、服务器已接受、失败、结果不确定；更新 Tauri 和内存 Adapter。

## 实施步骤

1. 明确阶段转换：queued、sending、accepted、retryable、failed、uncertain、cancelled；持久化尝试标识后才调用网络。
2. 有确定临时拒绝时退避重试；已经发送或阶段未知且没有确定接受/拒绝证据时进入 uncertain，lease 到期不自动重发。
3. 远端接受后先持久化接受证据；本地确认失败且进程仍在时只重试确认，重启无证据保持 uncertain。
4. 只有入队事务成功才清理对应草稿并返回已入队；失败保留内容。服务器接受不表示最终送达。
5. worker 首次启用即使用 Task 04 的账号受控入口；即时与周期触发均登记同一个 gate，退出、切换配置和清空数据等待实际尝试收尾。Task 15 负责统一调度，不补做本任务缺失的安全接线。

## 验收

- 离线、并发认领、重启、认证拒绝、DATA 最终应答丢失和接受后本地写失败。
- 重试发送内容和附件字节完全一致；UI 不把入队或不确定显示成发送成功。
- 在发送中退出/换凭据/清空数据：停止新派发，旧连接结束并记录必要结果后才允许账号变更；未知结果不会重发。
- 修改已读不出现“待发送”，删除意图不显示成远端已删除，服务器接受不把位置或归档状态改成已同步。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。

## 新增表和函数

- domain/entities/send_task.rs：发送阶段、尝试与恢复规则；infrastructure/repository/send_tasks.rs：enqueue、claim_due、mark_network_started、record_result、recover_interrupted；runtime/send_worker.rs：通过 Task 04 入口执行单次尝试。
- send_tasks 最低字段：task_id、account_id、message_id、content_version、snapshot_id、state、attempt_id、claim_token、lease_until、attempts、next_attempt_at、last_error_code、last_error、accepted_at。外键保证消息/内容属于同一账号。
- compose_use_case::send 的事务同时写消息、快照引用、附件引用、发送任务和账号版本；返回 queued 结果及 ChangeSet。outbound_use_case 执行一次已认领尝试，不再扫描 PendingLocal 决定发送。
- 认领发生在取得账号 permit 后；结果更新条件必须匹配 task_id、content_version、attempt_id、claim_token。lease 时间到期不撤销仍运行尝试的所有权，也不允许另一 worker 绕过 gate。
- 恢复 sending 时，先确认旧执行者已经结束，再按持久的网络阶段和接受证据决定结果；没有未接受证据时转 uncertain。保存接受证据失败时仅重试本地确认，不再调用协议发送。

## 拆除旧状态混用（本任务负责）

1. 修改 domain/value_objects/enums.rs、domain/entities/message.rs、001_init.sql 和 repository/messages.rs：移除用 PendingLocal/Synced/DeletedRemote 同时表示发送、删除和定位的规则。发送状态归 send_tasks；定位保留 present/missing/invalidated；文件夹同步状态继续独立保存。
2. 本地草稿归 Task 03 的草稿表；乐观已读/删除及待推意图归 mutation。Task 10/11 在此模型上补全恢复与移动行为，不再添加第二套消息同步状态。
3. 同步修改 src/domain/entities.ts、src/domain/types.ts、useMessages.ts、MailReadingPane.vue 以及消息/会话查询过滤。已读乐观更新只改展示值和待确认意图，不能设置 pending_local；删除不能伪造 deleted_remote 事实。
4. 删除以 sync_status 判断本地发送记录可见性的 SQL。普通会话按有效位置、角色和本地发送记录的明确范围判断；Task 14 补齐 Sent 副本关联，不恢复旧状态兼容分支。


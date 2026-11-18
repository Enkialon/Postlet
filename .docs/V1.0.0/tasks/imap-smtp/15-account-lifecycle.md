# Task 15：统一所有后台任务的账号生命周期

- 状态：已完成
- 实现备注：clear_local_data 先 quiesce（阻塞新派发并等待实际任务退出），再同步冲刷一次待推已读/删除，最后清库；发送、归档、聚合、mutation 各自独立 tick 循环，单类故障不饿死其他任务，均通过同一账号 gate 认领。归档中断恢复为 uncertain 且不可再被认领（有单测）。
- 对应问题：A-04、I-14
- 前置任务：04、08、10、11、13、14

## 修改位置

- `src-tauri/src/runtime/{accounts,mail_watchers,mail_sync_scheduler,aggregate_scheduler,mail_sync_gate}.rs`：在已有监听控制和 gate 上接入发送、mutation、归档、联系人和聚合任务的句柄、取消和退出等待。
- `src-tauri/src/application/use_cases/{message,outbound,aggregate_worker,account}_use_case.rs`：移除各自无法追踪的一次性后台启动；按独立任务返回错误与已提交结果。
- `src-tauri/src/infrastructure/{imap,smtp,graph}/`：复用任务 01 的端点和认证解析；Graph Delta 与 IMAP IDLE 仅作为各自适配器的唤醒/同步机制。

## 实施步骤

1. 启动和重新登录扫描已登录账号，收信、mutation、发送、归档、聚合分别可恢复；单类失败不能阻止其他类继续。
2. 复用并审计 Task 08/10/14 已接通的退出屏障，将剩余联系人和聚合工作接入同一流程。账号变更先阻止新派发，取消并等待实际任务退出，再执行变更；恢复时按各自持久阶段重建。
3. 保留 INBOX IDLE 和约 60 秒全已勾选夹兜底；使用现有具体模块和任务表，不引入通用调度框架。

## 验收

- clear_local_data 前必须停止并等待该账号 flush_due_mutations，与 sync 使用 Task 04/已有 gate 的同一退出屏障；禁止远端删除在清库后仍继续执行，不新增锁类型。

- Graph/IMAP 均覆盖离线重试、重启、退出、切换凭据和重新登录；重复触发不会产生并行旧任务。
- 一类任务故障不饿死其他任务；退出后无残留连接或继续写入。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。

## 与前置任务的边界

- Task 04 提供实际任务持有 permit、操作登记、取消和等待的基础；Task 08/10/14 在首次启用新 worker 时完成接线，不能等本任务才补退出保护。
- 本任务统一启动、登录、重连和周期唤醒策略，消除 Graph 专属 mutation 恢复过滤及无句柄启动，并接入剩余联系人/聚合流程，负责全类别交叉验收。
- 每类任务保留自己的恢复规则，不能将 sending/uncertain、未知移动或 APPEND 未确认统一重置为可重试。
- 复用 runtime/accounts.rs、mail_sync_gate.rs、mail_watchers.rs 和具体 worker；关键入口是启动扫描、登录恢复、pause_and_wait、受控单次执行和提交后分发，不另建通用调度框架。


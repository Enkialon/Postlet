# Task 04：移除用例对运行时和 AppState 的反向依赖

- 状态：已完成
- 实现备注：用例不再依赖 `runtime` / `AppState` / `tauri`；同步与聚合返回 `Option<ChangeSet>`，由 runtime/commands 在提交后分发。本地已读/删除通过 `ChangeSet.pending_remote_flush` 通知 runtime 立即冲刷远端，用例内不再 `spawn`。受控执行入口（`runtime/mail_sync.rs::run_owned`）已有取消/超时仍持有 gate 的测试。
- 对应问题：A-03、A-02
- 前置任务：01、02

## 修改位置

- `src-tauri/src/application/use_cases/{sync,aggregate_worker,message,compose,account}_use_case.rs`：将 Dispatcher、AppState 和 tauri::async_runtime::spawn 移出用例；显式接收数据库、路径和操作所需数据，返回业务结果与已提交 ChangeSet。
- `src-tauri/src/runtime/{mail_sync,mail_watchers,mail_sync_scheduler,aggregate_scheduler,change_dispatcher,accounts}.rs`：由 runtime 持有 gate、任务句柄、取消和提交后分发；现有 commands 继续作为薄 IPC 胶水。
- `src-tauri/src/application/dto/change_set.rs、src-tauri/tests/repository_transactions.rs`：调整现有结果契约和跨仓储事务测试；不增加 interfaces、Repository trait 或通用事件总线。

## 实施步骤

1. 逐个替换 application 中的 runtime、AppState 和 Tauri 引用。Task 03 已完成时传入 SQLite 资源；尚未完成时显式传入现有草稿资源，持久化仍归 Task 03，不新增过渡 trait。
2. 在事务内收集候选变更，只有 commit 成功才返回可分发的 ChangeSet；网络调用放在事务外。
3. 本任务定义提交结果与外部分发的边界，并接通现有单次提交。逐夹/逐批收信、下载候选和部分成功交付统一由 Task 13 实现，不在这里提前重写同步循环。
4. 将 runtime/mail_sync.rs 的现有 run_owned 整理为 crate 内可复用的账号受控执行入口，继续由实际任务持有 permit。新 worker 首次启用时必须通过该入口登记操作，不能等到 Task 15 才接入退出屏障。用例内部不再次申请同一账号 gate。

## 验收

- 检索 application 不再依赖 runtime、Tauri 和 AppState，infrastructure 仍不反向依赖 application。
- 跨仓储提交/回滚、手动和后台重入、commit 前失败无事件通过；多批部分成功验收归 Task 13。
- 受控执行入口测试：取消调用方或暂停账号后，实际 worker 未退出前 permit 不释放；收尾确认完成后才允许新任务进入。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。

## 用例与 worker 接线契约

- 用例返回明确的业务结果及成功提交的 ChangeSet；失败事务的候选摘要不交给 dispatcher。不新增泛型 UseCase、Repository 接口或事件总线。
- runtime 持有 JoinHandle、MailOperation 和账号 permit；即时触发和周期触发使用同一入口。进入后重新检查登录态与配置，再调用 application。
- runtime/accounts.rs 的账号变更屏障必须能取消该入口登记的活动操作，并等待实际退出；只有等待者取消不能证明 worker 已结束。
- Task 08/10/14 各自在首次派发时完成该接线和取消测试。SMTP/APPEND 已开始而结果未确认时，取消后必须先持久化不确定状态，再释放 permit；只允许必要的本地收尾，不继续远端写入。
- 本契约基于现有进程内 gate，不能宣称已经提供跨进程执行互斥；启用多应用实例前必须另落实执行互斥约束。


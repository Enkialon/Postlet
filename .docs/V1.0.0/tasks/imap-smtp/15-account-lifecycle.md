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

## IMAP 收信延迟：分阶段实施

状态：待实施。一次实测中，邮件收件时间为 `13:38:38`，定时兜底在 `13:39:26.449` 触发，`INBOX` 在 `13:39:27.224` 提交 SQLite，入库耗时 37 ms。这次延迟主要在同步触发前，但现有日志不能证明当时没有收到 IDLE 通知。先让通知可追踪、避免通知因账号繁忙而丢失；本方案不调整约 60 秒的全账号兜底，也不改变监听范围或历史补齐节奏。

### 阶段一：定义监听实体和入口

修改 `src-tauri/src/runtime/mail_watcher.rs` 的 `listen_folder()`，把通道里的 `(FolderId, String)` 换成文件夹变化事件。复用现有 ID、绑定版本和任务取消控制，不写数据库。

```rust
struct ImapWatchTarget {
    account_id: AccountId,
    folder_id: FolderId,
    remote_binding: String,
    binding_revision: i64,
}

struct ImapFolderChanged {
    target: ImapWatchTarget,
    observed_at: DateTime<Local>,
}

async fn listen_folder(
    db: SqlitePool,
    paths: AppPaths,
    target: ImapWatchTarget,
    tx: UnboundedSender<ImapFolderChanged>,
    control: WatcherControl,
);
```

- 继续调用 `infrastructure/imap/idle.rs::run_idle_until_cancel()`。只有 `IdleOutcome::Changed` 记录通知时间并发送事件；超时、断线不发事件。IDLE 通知表示文件夹有变化，不保证有新邮件。
- `watch_one_account()` 按监听目标管理任务。绑定版本变更时替换旧监听，旧绑定或已退役目标的排队事件不得触发同步。

### 阶段二：定义同步入口、请求与结果

在 `src-tauri/src/runtime/mail_sync.rs` 增加后台入口，复用现有 `run_owned()`、`MailSyncGate` 和 `sync_use_case::trigger_mail_sync()`；不新增 trait、IPC 或数据库字段。

```rust
// 类型定义在 runtime 内，由监听编排使用。
enum ImapSyncTrigger {
    Idle(Vec<ImapFolderChanged>),
    Fallback,
}

enum ImapSyncOutcome {
    Busy,      // 未取得账号 gate，本次未执行
    Finished,  // 同步用例已返回成功，不等于有新邮件
}

async fn try_sync_imap(
    db: &SqlitePool,
    paths: &AppPaths,
    changes: &ChangeDispatcher,
    gate: &MailSyncGate,
    account_id: &AccountId,
    trigger: &ImapSyncTrigger,
    operation: MailOperation,
) -> Result<ImapSyncOutcome, AppError>;
```

- `Idle` 将去重后的文件夹 ID 传给 `TriggerMailSyncCommand.folder_ids = Some(...)`；`Fallback` 传 `None`，沿用已启用文件夹筛选。
- `run_owned()` 的 `Ok(None)` 映射为 `Busy`，执行错误原样返回。手动同步仍沿用现有冲突反馈；不把 `Busy` 伪装为一次成功同步。
- 同步用例继续按文件夹提交并通知。`Finished` 只描述用例返回结果：是否写入新邮件以 `sync_use_case.rs::apply_imap_folder_batch()` 在 `tx.commit()` 之后的 `SQLite 已提交，新增=N` 为准；文件夹级错误仍沿用现有处理。

### 阶段三：在 `watch_one_account()` 组合事件与调度

```text
循环直到账号取消：
    等待 IDLE 事件、兜底到期或退避结束
    排空已到达的事件，丢弃失效目标；按 folder_id 合并到 pending
    同一目标保留最早通知时间，用于计算排队耗时

    若仍在退避：继续接收事件，不发起同步
    若兜底到期：请求 Fallback，不消费 pending
    否则若 pending 非空：取 pending 快照，请求 Idle(快照)
    否则：继续等待

    match try_sync_imap(请求)：
        Busy     → 恢复 IDLE 快照；保留到期的兜底请求；短暂退避
        Finished → 仅消费本次 IDLE 快照；若是兜底则更新下一次到期时间
        Err      → 记录错误，恢复 IDLE 快照；保留兜底请求并退避

    执行期间新到的事件留在通道里，下轮处理
账号取消：停止监听，等待受控操作收尾
```

兜底成功后按现有规则选择历史续跑间隔或正常兜底间隔。日志记录账号、文件夹、触发来源、通知时间、同步开始时间或 `Busy`，以及 SQLite 提交时间；不记录邮件内容。验收：有通知时只同步目标文件夹且不等兜底；gate 忙时通知不丢；无通知时兜底仍可入库；重绑定后旧通知不会触发同步。

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


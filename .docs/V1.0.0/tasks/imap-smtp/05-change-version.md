# Task 05：持久化账号变更版本并恢复前端刷新

- 状态：已完成
- 实现备注：`accounts.change_version` 由写事务内 `accounts::bump_change_version` 递增，`ChangeSet.account_version` / `MailChangedEvent.account_version` 携带持久版本；IPC `get_account_change_version`。前端按账号记录已应用版本，事件与窗口焦点校验版本后重查，过期结果不覆盖。回滚不增版本有仓储测试。
- 对应问题：A-03
- 前置任务：04
- 剩余范围：基线已有 ChangeSet 和进程内事件分发；本卡只补持久账号版本、同快照查询及前端补查，不重做既有分发通道。

## 修改位置

- `src-tauri/migrations/001_init.sql、src-tauri/src/infrastructure/repository/accounts.rs`：为账号增加单调 change_version 和事务内递增函数；清空账号数据时保留并递增版本。
- `src-tauri/src/application/dto/change_set.rs、src-tauri/src/runtime/change_dispatcher.rs`：ChangeSet 和事件携带已提交的账号版本；删除以进程内 AtomicU64 作为持久顺序的做法。
- `src/ports/changeEventsPort.ts、src/ports/{account,session,message}Port.ts、src/adapters/{tauri,memory}/、src/features/{accounts,conversations}/`：查询数据和版本使用同一读快照；启动、重订阅、恢复焦点及低频检查版本并按账号重查。

## 实施步骤

1. 所有影响前端查询的事务，包括消息、会话、任务状态和清空数据，在同一事务递增版本；后续任务新增的写事务也必须调用该函数。
2. 前端按账号保存已应用版本；发现跳号或范围不可恢复时重查账号，较旧查询结果不能覆盖较新状态。
3. 事件仅作为刷新提示；系统通知保持尽力而为，不引入通知 outbox。

## 验收

- 事务回滚不增版本、不发事件；commit 后通知前退出，重启查询可恢复。
- 模拟漏事件、重订阅、焦点恢复和查询乱序；两批成功第三批失败时前两批仍可刷新。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。


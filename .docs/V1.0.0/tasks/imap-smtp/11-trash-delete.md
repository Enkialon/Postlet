# Task 11：默认删除移入垃圾箱，永久删除保持单封安全

- 状态：已完成
- 实现备注：默认删除写入 `kind='trash'` 意图并冻结源位置 + 目标 Trash 文件夹（无 Trash 角色时不写入、不扩大操作）；`kind='delete'` 仅由显式永久删除触发，保留既有 UIDPLUS/UID EXPUNGE 限制。IMAP 优先 MOVE，缺失时 COPY + 单封 UID EXPUNGE，任一步未知都返回明确错误而不回退整夹清理；Graph 用 move 原子移动。前端 `permanent` 参数区分两种语义。
- 对应问题：I-01、I-10
- 前置任务：09、10
- 剩余范围：UIDPLUS/UID EXPUNGE 的单封安全删除止损已提交；本卡新增默认移入 Trash 的语义与接线，永久删除原语仅保持回归，不重新实现。

## 修改位置

- `src-tauri/src/domain/entities/message_mutation.rs、src-tauri/src/application/use_cases/message_use_case.rs`：区分默认移入 Trash 和用户明确选择的永久删除；冻结源位置、目标 Trash 绑定和相关修订。
- `src-tauri/src/infrastructure/imap/client.rs、src-tauri/src/infrastructure/graph/mail.rs`：实现受控移动；IMAP 优先 MOVE，必要时在能力足够时使用安全 COPY + 指定 UID 删除；Graph 操作限定源文件夹。
- `src/ports/messagePort.ts、src/adapters/tauri/、src/features/conversations/`：提供默认删除及明确的永久删除操作；能力不足、目标未确认或结果未知时显示可处理状态。

## 实施步骤

1. 目标必须是已确认的 Trash；角色、绑定或源位置变更后停止旧任务，不重新选目标。
2. 移动确认后按源位置移出、目标位置新增入库；目标实体不能继承源 MessageId、已读或删除任务。
3. 永久删除只允许明确选择当前位置；保留已有 UIDPLUS 与 UID EXPUNGE 安全限制，绝不回退整夹 EXPUNGE。
4. MOVE/COPY 最终结果未知时核对源和目标；证据不足标不确定，不自动重复可能产生副本的操作。

## 验收

- 无 Trash、无安全能力、UIDVALIDITY 改变时没有扩大操作；Trash 内永久删除必须显式选择。
- 覆盖 MOVE 成功、COPY 后失败、断线未知、远端成功本地回滚；同内容其他位置不受影响。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。


# Task 03：持久化草稿和受管理附件

- 状态：已完成
- 对应问题：I-07
- 前置任务：01
- 范围边界：为 I-08 发送队列提供持久内容引用；队列、派发和发送状态本身归 Task 08。
- 实现备注：草稿与附件内容落 SQLite，二进制落 `attachments/{account_id}/`；同账号内容安装、引用变更与回收共用附件锁，`try_lock` 忙时返回可重试 Conflict。回收互斥验证改为确定性持锁测试（先持锁再 save 断言 Conflict），不再保留仅供测试的回收钩子。

## 修改位置

- `src-tauri/migrations/001_init.sql、src-tauri/src/infrastructure/repository/`：新增草稿、附件内容及引用的最小表和仓储模块；消息、草稿和以后发送任务通过稳定引用关联内容。
- `src-tauri/src/infrastructure/paths.rs、src-tauri/src/infrastructure/`：在现有 attachments 目录实现导入、读取、引用检查与回收模块；临时文件和最终文件位于同一受管理文件系统。
- `src-tauri/src/application/dto/compose_dto.rs、src-tauri/src/application/use_cases/compose_use_case.rs`：附件 DTO 改为稳定内容引用；用 SQLite 替代仅存在于 AppState 的草稿，保存正文、收件人和附件引用。
- `src/ports/composePort.ts、src/adapters/{tauri,memory}/、src/features/compose/useComposeDraft.ts`：增加附件导入 Port，先由 Rust 导入内容再写入草稿；内存适配器实现同一契约。

## 实施步骤

1. 导入时将字节写临时文件，校验账号归属、长度和内容完整性，完成必要持久化后原子移动；SQLite 只保存可验证的稳定引用。
2. 草稿或消息引用在事务内提交；失败清理未引用文件。启动时核对暂存、孤儿和缺失内容；回收前同时检查所有持久引用和运行中的使用者。
3. 草稿删除、取消、消息删除和退出按引用决定回收；已有附件发送拒绝保持到任务 08 完整接通。

## 验收

- 在回收完成引用检查后暂停回收线程，同时提交新草稿引用；验证互斥使两者不能交错成“有效引用指向已删除文件”。

- 同名、二进制和仅附件草稿重启后内容一致；模拟临时写、移动和 SQL 失败。
- 缺失文件可见报错；两个消息共享引用时删除一个不会破坏另一个；导入、发送与回收并发不误删。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。

## 新增模块与数据契约

以下为本任务拟新增名称，实施时在这些模块中落地，不只创建目录：

- infrastructure/attachment_store.rs：临时写入、长度/摘要校验、原子安装、受控读取和删除；返回稳定 content_id，不把前端原始路径作为长期引用。
- infrastructure/repository/attachments.rs：内容及引用查询；application/use_cases/attachment_use_case.rs：组合文件操作和 SQL 事务。通过现有 ComposePort 增加 importAttachment，Tauri Adapter 负责调用命令。
- attachment_contents：content_id、account_id、storage_key、byte_length、digest、created_at。内容就绪后不可覆盖。
- compose_drafts：draft_id、account_id、session_id、正文/收件人、updated_at。draft_attachments、message_attachments 保存各自 owner_id、content_id、名称、MIME 和顺序；外键约束同账号归属。发送/归档的内容引用分别由 Task 08/14 加入。
- 最小函数：import_attachment、save_draft_with_refs、load_content、collect_unreferenced。查询/引用变更由 repository 模块函数完成，跨仓储事务由 application 控制。

## 引用提交与回收互斥

1. 同账号的内容安装、增删引用和回收共用附件锁；锁文件放在该账号受管理附件目录，进程内调用与进程间调用遵守同一锁。统一先取附件锁，再开启写事务，避免反向等待。忙时返回可重试冲突，不删除文件。
2. 导入和保存引用持锁覆盖最终内容安装、引用 SQL 提交及失败清理。独立导入先创建持久的草稿附件引用，再返回给前端；不能在返回后留下仅靠前端内存保护的内容。
3. 回收持锁覆盖引用检查、最终文件删除和内容行清理；删除前检查草稿、消息、发送快照、发送任务、归档任务以及运行中持有者。文件已删除但 SQL 清理失败时，重启核对继续清理无引用行；不能留下有效引用。
4. 网络调用期间不持有附件锁。先在锁内登记任务的持久内容引用，再释放锁读取/发送；任务结束并完成必要归档前不得释放其内容所有权。仍在读取的内容不得因任务状态变化而提前解除保护。
5. 回收失败保留待清理状态；账号退出只停止使用者，不无条件删除仍被持久引用的内容。


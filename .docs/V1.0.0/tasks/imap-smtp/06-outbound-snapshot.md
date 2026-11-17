# Task 06：生成可重放的发送内容和正确回复头

- 状态：已完成
- 实现备注：发送前生成并持久化不可变快照（`outbound_snapshots` + `snapshot_attachments` + 受管理 MIME 内容）。Message-ID 固定、Date 由构建时写入 MIME 字节、References 至少含父信 ID（完整链待 MIME 摄取任务）。Bcc 仅进 envelope。带附件发送仍被拒绝以等待队列任务；快照构建与持久化失败会回滚并保留草稿。
- 对应问题：I-07、I-08、I-11、A-06
- 前置任务：03、04

## 修改位置

- `src-tauri/src/application/use_cases/compose_use_case.rs、src-tauri/src/domain/entities/message.rs`：发送前固定本信 Message-ID、Date、正文、收件人和父信引用；回复使用父信 in_reply_to 并构造 References。
- `src-tauri/src/infrastructure/smtp/send.rs、src-tauri/src/infrastructure/graph/mail.rs`：提取实际使用的序列化函数，生成可重复读取的 MIME 和发送 envelope；Graph 发送模型读取相同正文及附件内容。
- `src-tauri/src/infrastructure/repository/、src-tauri/migrations/001_init.sql`：保存不可变 MIME/内容引用、envelope 和附件引用；Bcc 仅进入 envelope，不泄漏到可见收件头。

## 实施步骤

1. 纯文本来源使用完整正文，不能使用截断预览；同时保留 HTML、文本和附件字节。
2. 首次入队前生成一次 Message-ID、Date、boundary 和 MIME，重试复用同一内容；编辑未派发内容时取消旧任务并建立新版本。
3. 附件读取或 MIME 持久化失败直接报错并保留草稿；本任务先接通生成和持久化，网络派发由任务 08 处理。

## 验收

- 长文本、回复头、多个收件人/Bcc、同名和仅附件 MIME 解析正确；连续读取字节、Message-ID、Date 完全相同。
- 注入附件缺失和内容持久化失败，不入队、不清空草稿、不产生不完整邮件。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。

## 新增模块与持久化契约

- 新增 infrastructure/mail/outbound_content.rs 的 build_snapshot，以及 infrastructure/repository/outbound_snapshots.rs 的 insert_snapshot、get_snapshot。
- outbound_snapshots 最低字段：snapshot_id、account_id、content_version、internet_message_id、date、mime_content_id、envelope_json、完整正文及收件人快照。snapshot_attachments 保存 content_id、名称、MIME、顺序，与 Task 03 的受管理内容关联。
- MIME 字节使用 Task 03 的内容存储和引用保护；快照持有持久引用，失败清理遵守同一附件锁，不新增第二套文件回收流程。
- 本任务只创建不可变快照，不新增可派发消息或任务，不绕过 Task 08 直接调用 flush_pending_message。关联消息、任务及账号版本的入队事务归 Task 08。
- Bcc 属于不可变 envelope：SMTP 用它生成 RCPT，不写公开 MIME 头；Graph 仍须将它映射为 bccRecipients，不能漏发。


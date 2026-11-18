# Task 12：完善 MIME、附件、地址组和日期解析

- 状态：已完成
- 实现备注：正文递归排除 attachment/message 部分；附件按 Content-Disposition/filename/CID 收集为统一 `RemoteAttachment`，`has_attachments` 由实际附件决定；`received_at` 优先 INTERNALDATE、`sent_at` 取 Date；地址组（RFC 5322 Group）展开为成员。附件内容在消息事务内写入受管理存储并建立 `message_attachments` 引用，失败即中止不留下悬空引用。
- 对应问题：I-12、I-07
- 前置任务：03、05
- 剩余范围：基线已有递归 MIME 遍历和基本预览；本卡补正文/附件选择、has_attachments、地址组及日期语义，不重做解析框架。

## 修改位置

- `src-tauri/src/infrastructure/imap/parse.rs、src-tauri/src/infrastructure/imap/sync.rs`：按 multipart/alternative、mixed、related 和 Content-Disposition 区分正文、普通附件及 inline/CID；展开地址组。
- `src-tauri/src/infrastructure/graph/mail.rs、src-tauri/src/domain/mail.rs`：将两种协议的正文、附件元数据和可获取内容映射为统一事实；协议专有字段只留在适配器。
- `src-tauri/src/application/use_cases/sync_use_case.rs、任务 03 的附件仓储`：先导入附件内容，再在消息事务中保存稳定引用；失败保留可见的下载错误，不伪称内容可用。

## 实施步骤

1. 文本或 HTML 附件不能被选为正文；related 的 CID 与普通附件分开记录，嵌套 message/rfc822 不误当主正文。
2. sent_at 取 Date；received_at 优先 IMAP INTERNALDATE，缺失时按文档回退；预览使用可靠的 HTML 文本和实体转换。
3. 接收端与 Compose 共用受管理内容生命周期；暂不新增不安全的 HTML 附件预览。

## 验收

- IMAP 解析后正确设置 has_attachments：存在普通附件或可下载 part 时为 true，不再恒为 false；使用现有解析结果，不另建附件索引系统。

- 嵌套 MIME、文本/HTML 附件、CID、编码、地址组、无 Date 和 INTERNALDATE 分别覆盖。
- 导入失败不产生有效内容引用；附件下载后字节与原始内容一致。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。


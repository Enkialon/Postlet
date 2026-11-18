# Task 09：识别特殊文件夹及控制展示范围

- 状态：已完成
- 实现备注：`mail_folder_sync_state.role/role_source`（server > manual > guess）；IMAP 解析 LIST SPECIAL-USE、Graph well-known 在 `infrastructure/graph` 内映射为同一角色；IMAP 名称/中文名仅作 guess。普通会话列表与未读计数排除 trash/junk/drafts/all（`include_excluded_folders` 才显示）；垃圾夹消息打开时不主动拉正文，阅读窗默认不渲染并提供「仍要查看正文」。
- 对应问题：I-12
- 前置任务：05
- 剩余范围：基线已有本地 FolderId、远端绑定及退役机制；本卡只补统一角色、来源和展示范围，为后续 Trash 删除提供角色，不实现删除。I-01 的删除语义归 Task 11。

## 修改位置

- `src-tauri/src/domain/entities/sync_state.rs、src-tauri/migrations/001_init.sql、src-tauri/src/infrastructure/repository/folder_states.rs`：保存 Inbox/Sent/Drafts/Trash/Junk/Archive/All/Custom、角色来源及必要的角色修订。
- `src-tauri/src/infrastructure/imap/client.rs、src-tauri/src/infrastructure/graph/mail.rs`：IMAP 映射 SPECIAL-USE 和 INBOX 语义；Graph 将远端目录映射到同一角色。
- `src-tauri/src/infrastructure/repository/{messages,sessions}.rs、src/features/accounts/FolderSyncSetupDialog.vue、src/features/conversations/`：按角色过滤普通会话和未读；提供缺失角色的手动指定及冲突确认。

## 实施步骤

1. 服务器角色优先，缺失时允许手动指定；名称猜测只作为待确认建议，不以语言或邮箱品牌硬编码身份。
2. 区分 Draft FLAG 和 Drafts 文件夹：普通范围排除 Drafts、Trash、Junk 以及 Draft/Deleted 标志；All 保留实体但默认排除，只在显式范围显示。
3. 角色变化重算展示投影并校验受影响任务；不改变 MessageId，不合并相同内容。

## 验收

- 有/无 SPECIAL-USE、非英文名称、角色变化、手动选择与冲突均有明确结果。
- INBOX、Archive、All 的同内容消息独立；普通范围只显示前两者，All 范围可显示第三者。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。


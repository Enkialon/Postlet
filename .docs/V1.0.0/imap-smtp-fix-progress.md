# IMAP / SMTP 修复记录

依据 `imap-smtp-architecture-review.md` 的 2026-11-17 工作区版本实施。原问题文档及规则已有未提交修改，本记录单独维护修复证据。仅完成对应验收后标为解决；止损不代表完整问题解决。

## I-01：已解决

- 删除前要求 UIDPLUS；能力缺失时不 SELECT、不 STORE、不产生删除副作用。
- 仅对目标 UID STORE / UID EXPUNGE；任何错误均不回退整夹 EXPUNGE。
- STORE 应答不确定及 UID EXPUNGE 失败明确报告目标可能/已经标记 Deleted，由现有 mutation last_error 保留。
- 验证：`cargo test --manifest-path src-tauri/Cargo.toml --lib infrastructure::imap::client::tests -- --nocapture`，2 项通过。模拟不支持 UIDPLUS、UID EXPUNGE 成功、NO、BAD、断线，核对命令范围。未连接真实邮箱。

## I-16：已修复，Windows 已验证

- 通用凭据和 Graph token 共用 AES-256-GCM 加密存储；随机 nonce、账号/用途认证绑定，系统钥匙串保管密钥，磁盘不保存明文。密钥缺失不静默替换已有密文的密钥；首次创建用跨进程文件锁串行。
- 临时密文写入并 sync 后原子替换；不读取旧明文开发凭据，无兼容分支。旧开发 JSON 需自行清理并重新登录，未自动删除已有文件。
- 秘密实体、认证参数、IPC 凭据 DTO、token 响应移除 Debug；退出删除通用凭据失败返回错误，可以重试。
- 移除未调用的扫描 JSON 推断登录账号入口；登录态仍以 SQLite 为准。
- 验证：13 项 Rust 单元测试通过；另外 `cargo test --manifest-path src-tauri/Cargo.toml --lib infrastructure::secret_store::tests --offline -- --include-ignored` 3 项通过，包含真实 Windows 凭据管理器隔离密钥的创建/读写/删除，以及大 token、替换、密文篡改、跨账号拒绝和失败保留测试。未验证 macOS/Linux 钥匙串及断电持久性。

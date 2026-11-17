# Task 01：完成收发认证拆分并恢复构建

- 状态：已完成（已提交：`99947e1` / `37e7133` 等收发认证相关提交）
- 对应问题：A-05、I-17
- 前置任务：无

## 修改位置

- `src-tauri/src/domain/entities/mail_transport.rs、src-tauri/src/domain/value_objects/credential_ref.rs`：完成 ReceiveConfig、SendConfig、受校验端点和凭据引用；IMAP/SMTP 密码认证分别包含用户名和引用，Graph 使用 OAuth 引用。
- `src-tauri/migrations/001_init.sql、src-tauri/src/infrastructure/repository/mail_transports.rs`：完成收发协议、端点、用户名和凭据引用的读写及 CHECK 约束；无效端口、明文 TLS、空引用和协议与字段不匹配不能落库。
- `src-tauri/src/application/use_cases/mail_account_use_case.rs、account_use_case.rs、src-tauri/src/infrastructure/mail/credentials.rs`：补完两个引用的暂存、事务切换、失败清理、旧引用回收和启动孤儿恢复；显式共用引用时只需要一份秘密。
- `src-tauri/src/application/use_cases/{sync,message,outbound}_use_case.rs、src-tauri/src/runtime/{mail_watcher,mail_watchers,mail_sync_scheduler}.rs`：按 receive/send 配置选择对应认证；更新当前残留的旧字段和 Graph token 调用，保留 Graph 登录与协议切换。
- `src-tauri/src/application/dto/commands.rs、src/domain/dto.ts、src/features/accounts/AddAccountDialog.vue、src/adapters/memory/MemoryAccountAdapter.ts`：统一新 IPC 参数；界面默认发信复用收信登录信息，用户选择单独配置时再填写 SMTP 用户名和密码；密码原样传递。

## 实施步骤

监听文件以现有实际职责为准：runtime/mail_watchers.rs 是任务表、配置核对与暂停/恢复入口；runtime/mail_watcher.rs 是单账号循环，由前者启动。两者不是替代版本，不另建并行入口；本任务只更新相应配置/认证调用。

1. 先保留当前未提交改动，逐个接通所有编译错误；不要新增旧字段兼容分支。
2. 校验两个端点和认证输入后再写秘密；分别保存新秘密，任一保存或 SQL/COMMIT 失败时仅删除确认未被引用的新秘密。
3. 同一事务切换账号和两个配置；成功后按命名空间回收不再引用的旧秘密，用户退出时检查两个引用并避免误删共享引用。
4. 通用 OAuth2 输入在反序列化或入口明确拒绝；Graph OAuth 继续使用已经实现的随机引用及配置锁。

## 验收

- 收发协议均切到 IMAP/SMTP 后，删除该账号已不再被配置引用的 graph 命名空间 token；recover 同时扫描 graph 孤儿。仍被任一有效配置引用的秘密必须保留，不新增远程 revoke 流程。
- 若认证接线触及连接调用，回归 I-15：登录成功后使用 session IO 预算/操作 deadline，不能继续把登录 deadline 当成整段同步期限；取消后旧连接退出再放行，不重写已有连接改造。

- IMAP 和 SMTP 不同用户名、不同密码可分别加载并路由；共用模式只保存一份凭据。
- 非法配置和通用 OAuth2 无数据库、凭据文件、网络副作用；模拟第二个秘密写失败、SQL/COMMIT 失败及重启清理。
- Graph→IMAP→Graph 切换和退出清理通过；运行 Rust 全套与 pnpm build。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。


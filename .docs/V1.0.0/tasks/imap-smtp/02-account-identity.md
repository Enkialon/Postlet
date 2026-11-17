# Task 02：移除统一账号模型中的 Graph 专有身份

- 状态：已完成（本提交移除 Account 的 tenant_id / home_account_id）
- 对应问题：Account Graph 登录元数据清理（A-01 的账号投影残留）
- 前置任务：01
- 剩余范围：基线已有统一邮件位置模型；本卡只清理 Account 的 Graph 登录元数据。A-02 已在修复进度中解决，仅回归确认，不重做依赖方向。

## 修改位置

- `src-tauri/src/domain/entities/account.rs、src-tauri/migrations/001_init.sql、src-tauri/src/infrastructure/repository/{accounts,rows}.rs`：从通用 Account 和通用账号表移除 tenant_id、home_account_id 及按 home_account_id 查找的仓储函数。
- `src-tauri/src/application/use_cases/account_use_case.rs、src-tauri/src/infrastructure/graph/{auth,models,token_store}.rs`：Graph 登录按现有规范化邮箱账号规则查找；Graph 登录所需的 provider 元数据留在 Graph 认证缓存内部，不作为邮件或账号业务身份。
- `src/domain/{entities,dto}.ts、src/adapters/memory/MemoryAccountAdapter.ts、账号相关测试`：同步删除前端和测试中不需要的专有字段；不保留旧 DTO 别名。

## 实施步骤

1. 梳理字段引用，删除只为通用账号投影服务的字段和 SQL；允许重建 V1 开发库。
2. 保留本地 AccountId、邮箱和用户维护的别名；同位置幂等仍使用账号、文件夹和 RemoteKey。
3. Graph 元数据变化不得使文件夹、Message 或任务换身份；协议切换仍走已实现的位置退役事务。

## 验收

- 同邮箱重新授权且 Graph 元数据变化时复用账号和现有位置；不同邮箱不因相同 provider 字段合并。
- 跨账号、跨文件夹相同内容仍为独立 Message；Rust 测试和前端构建通过。

完成并通过相关验证后，将本任务涉及的代码和测试单独提交；更新总览状态。


# V1.0.0 后端未完成工作设计

本文记录当前后端已落地能力、明确不做的范围，以及接入真实 Outlook / Graph 前的剩余设计。契约细节仍以 [ports-entities.md](./ports-entities.md)、[data-model.md](./data-model.md) 为准；本文只补「还差什么、放哪里、怎么串」。

相关前端规格：[frontend.md](./frontend.md)。Agent 约束见根目录 `AGENTS.md`。

---

## 1. 当前基线（已完成）

| 能力 | 状态 | 说明 |
|---|---|---|
| SQLite schema + migrate | 已有 | `src-tauri/migrations/001_init.sql` |
| 本地目录布局 | 已有 | 配置 `app_config_dir`；邮件库 `app_local_data_dir/data/postlet.db`；预留 `attachments/`、`credentials/`；旧库路径可迁移 |
| 本地读路径 | 已有 | 账户 / 联系人 / 会话列表（含预览）/ 会话消息分页 / 邮件详情 / 群组只读 / 同步进度只读 |
| 本地写（有限） | 已有 | `mark_read`（仅本地 + 重算投影）、`update_display_settings`、仓储 upsert/attach |
| 演示种子 | 已有 | `load_demo_workspace` 镜像前端 `seedDemo`，不接 Graph |
| Tauri IPC（读 + demo） | 已有 | `commands/*`；前端 Tauri Adapter 对接程度以当时前端为准 |
| Domain 聚合纯函数 | 已有骨架 | `decide_aggregation` 等；Worker 尚未真正领取 job |

**刻意保持单库**：联系人 / 会话 / 邮件同库；不按业务拆 SQLite。附件外置、日后可按账户分文件，见 §7。

---

## 2. 架构约定（讨论结论）

### 2.1 依赖方向

```text
前端 Port → Adapter → Tauri command
        → application use_cases
        → infrastructure（repository / graph）
        → domain（纯函数）

runtime（调度）→ application     ✅
commands / lib setup → application ✅
infrastructure → application     ❌ 禁止
```

### 2.2 真实邮件的两道产品入口

| 入口 | Port / 用例 | 职责 |
|---|---|---|
| 登录 | `AccountPort.signIn` → `account_use_case::sign_in` | OAuth、账户落库、token 写入 `credentials/` |
| 拉信 | `SyncPort.triggerMailSync` → `sync_use_case::trigger_mail_sync` | 触发一轮 Delta；成功≠同步完成 |
| 读 UI | Conversation / Message / Contact Port | **只读本地库**，不直连 Graph |

Graph 调用只允许出现在 `infrastructure/graph/*`（`graph-rs-sdk`）。

### 2.3 定时调度器放哪里

**必须在 Rust 进程内**，不能放前端。

- 推荐模块：`src-tauri/src/runtime/`（与 `commands` 同级的组合根旁模块）
  - `mail_sync_scheduler.rs`：按账户间隔触发 `trigger_mail_sync`
  - `aggregate_scheduler.rs`：定时 `process_available_jobs`
- 在 `lib.rs` `setup` 中 `spawn`，持有 `SqlitePool`（或 `AppState` 句柄）
- **不要**放进 `infrastructure/`，否则会出现 infra → application 反依赖
- 前端仅允许轮询 `getMailSyncProgress` / 列表刷新（展示用），不当拉取引擎

手动「立即同步」与定时器共用同一 use case。

### 2.4 邮件处理形态：不用责任链

采用 **用例编排 + Domain 纯函数 + 聚合任务状态机**：

```text
sync 一页 Delta
  → 映射 Remote* → 本地实体
  → 事务：upsert + 游标 + enqueue job
  →（事务外）locate / 补字段

aggregate worker
  → 租约领取
  → 读快照 → decide_aggregation
  → 事务：apply plan（附着 / 群组 / 投影）+ complete
```

- 无 Handler 责任链、无可替换 Policy trait 对象  
- 缺 ThreadId → `Partial`；无可执行动作 → `Skipped`；可恢复缺失 → `Deferred` + retry  
- 显式顺序的纯步骤函数可以拆，但是 **编排**，不是责任链  

---

## 3. 未完成工作清单

### 阶段 A — 身份与密钥（接入微软的前提）

| 项 | 位置 | 说明 |
|---|---|---|
| OAuth 登录 / 退出 | `graph/auth` + `account_use_case::{sign_in,sign_out}` | 浏览器/系统流程；刷新 token |
| Token 存储 | `credentials/` 或系统钥匙串 | 禁止明文长期塞进业务表 |
| 账户 upsert 与本地 AccountId 绑定 | auth → accounts 仓储 | 登录不得覆盖用户显示配置 |

**验收**：`signIn` 返回本地 Account；重启后 `listAccounts` 仍可用；token 可刷新。

### 阶段 B — 同步写路径（真邮件进库）

| 项 | 位置 | 说明 |
|---|---|---|
| 邮件 Delta | `graph/mail::delta_messages` + `sync_use_case::trigger_mail_sync` | 按文件夹游标；页内变更与游标原子提交 |
| 移出核对 | `locate_message` + 核对记录 | Remove ≠ 永久删除；版本匹配才生效 |
| 联系人 Delta | `graph/contacts` + `trigger_contact_sync` | 更新通讯录缓存 |
| 正文按需 | `get_message_body` + `ensure_message_body` | 本地 `body_content == null` 时拉取并回写 |
| 已读推远端 | `mutations` + `patch_message_read` | 本地先成功；串行推送；与 Delta 乱序保护 |

**验收**：对真实账户触发同步后，会话列表 / 消息分页出现远端邮件；进度 DTO 反映 idle/running/error。

### 阶段 C — 聚合 Worker

| 项 | 位置 | 说明 |
|---|---|---|
| jobs 仓储 | `repository/jobs` | enqueue / claim / complete / retry / fail / 租约过期 |
| `aggregate_message` / `process_available_jobs` | `aggregate_worker_use_case` | 校验 generation + lease 后调 `decide_aggregation` |
| 配置变更重投影 | 显示开关变更后 | 受影响消息重新入队 |

**验收**：同步入队后，无需前端参与，会话投影与未读数收敛；崩溃后租约可恢复。

### 阶段 D — 运行时调度

| 项 | 位置 | 说明 |
|---|---|---|
| `runtime` 模块 | `src-tauri/src/runtime/` | mail sync tick + aggregate drain |
| `lib.rs` spawn | 组合根 | 启动挂载、退出停止 |
| 退避与跳过 | scheduler 配置 | `running` 中不重入；错误指数退避；可先固定间隔 |

**验收**：应用空闲时仍周期性推进同步与聚合；关掉前端页面不影响 Rust 循环。

### 阶段 E — 群组写与会话 ensure

| 项 | 位置 | 说明 |
|---|---|---|
| 手动建组 / 重命名 / 合并 / 挪线程 / 发信成员 | `group_use_case` | 同事务迁移附着与两侧投影 |
| `ensure_contact_conversation` / `ensure_group_conversation` | `conversation_use_case` | 占位联系人与根群组解析 |

### 阶段 F — 写信（若 V1 纳入）

| 项 | 位置 | 说明 |
|---|---|---|
| Compose 草稿持久化 | 新表或本地文件 + Port | 前端已有 Compose Port 方向 |
| `send` | graph send + 本地 outbound 落库 + 入队聚合 | 本阶段前可用 Memory；Rust 未接 |

（以产品范围为准；`frontend.md` 曾将写信列为 V1 不提供，若范围变更则升为本表正式项。）

### 阶段 G — 前端接线

| 项 | 说明 |
|---|---|
| `createTauriPorts` 全量 invoke | command 名与参数与 `commands` 对齐 |
| 登录成功后触发同步 | UI 调 Port，不自己 setInterval 拉 Graph |
| `loadDemoWorkspace` 与真账户并存策略 | 开发开关或菜单入口 |

---

## 4. 建议实现顺序

```text
A 登录/Token
  → B1 联系人 Delta（标题更好）
  → B2 邮件 Delta 落库 + 入队
  → C jobs + aggregate worker
  → D runtime 双调度器
  → B3 正文按需 / 已读推送
  → E 群组写操作
  → G 前端 Tauri Adapter 收尾
```

每一步都应保持：**UI 只读本地库**；失败可重试；不在 command 里写 Graph 细节。

---

## 5. `runtime` 模块草图

```text
src-tauri/src/runtime/
  mod.rs
  mail_sync_scheduler.rs    # interval → list_accounts → trigger_mail_sync
  aggregate_scheduler.rs    # interval → process_available_jobs(limit)
```

伪代码（组合根）：

```rust
// lib.rs setup 内
let db_mail = state.db.clone();
tauri::async_runtime::spawn(async move {
    runtime::mail_sync_scheduler::run(db_mail).await;
});
let db_agg = state.db.clone();
tauri::async_runtime::spawn(async move {
    runtime::aggregate_scheduler::run(db_agg).await;
});
```

调度器 **只调 application**；不 import `infrastructure::graph`。

---

## 6. 同步与聚合的事务边界（实现时必须遵守）

1. **一页邮件 Delta**：消息变更 + 该文件夹 `next_link`/`delta_link` + 本页相关 aggregate 入队，同一事务。  
2. **聚合一条 job**：校验 claim → 读上下文 → `decide_aggregation` → 写投影/附着 → complete，同一事务；旧租约结果不得覆盖。  
3. **网络 IO 在写事务外**：Graph 拉取、locate；回写再开短事务。  
4. **账户级串行写远端**：同一账户 PATCH 已读等操作排队，避免与 Delta 乱序互相覆盖（见 ports-entities §4.10）。

---

## 7. 存储与性能（已定方向）

| 决策 | 内容 |
|---|---|
| 配置 vs 数据 | 配置走 `app_config_dir`；SQLite 走 `app_local_data_dir/data/` |
| 分库 | V1 **不分业务库**；单库 + 索引 + 会话投影 |
| 附件 | 不进主表大字段；目录 `attachments/` |
| 日后扩展 | 库体积过大时优先「每账户一个 db 文件」，而非 contacts/messages 拆库 |

---

## 8. 非目标（本文不展开实现）

- 多厂商邮箱 / IMAP 通用层  
- 前端直连 Graph  
- 以文件夹树为主导航  
- 服务端中转  
- 为分库而分库、责任链式邮件 Pipeline  

---

## 9. 与现有骨架的映射（便于开工）

| 未完成点 | 现有空洞 |
|---|---|
| 登录 | `account_use_case::sign_in/out`，`graph/auth::*` |
| 拉信 / 通讯录 | `sync_use_case::trigger_*`，`graph/mail|contacts` |
| 聚合 | `aggregate_worker_use_case`，`repository/jobs` |
| 已读推送 | `repository/mutations`，`graph/mail::patch_message_read` |
| 群组写 | `group_use_case` 多个 `not_implemented` |
| ensure 会话 | `conversation_use_case::ensure_*` |
| 调度 | **尚无** `runtime/` 目录，需新建 |

演示路径 `load_demo_workspace` 在真邮件落地后保留，用于无网络验收。

---

## 10. 文档维护

- 实现某阶段时：更新本文件对应表格状态，并回写 ports-entities / data-model 若有契约变更。  
- Command 名称定稿后：同步 `frontend.md` §10 与 Tauri Adapter。  

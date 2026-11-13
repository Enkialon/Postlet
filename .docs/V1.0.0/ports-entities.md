# V1.0.0 接口与实体规格

基于 [`data-model.md`](./data-model.md) 与 [`AGENTS.md`](../../../AGENTS.md)。

**本文只定义**：目录落点、实体 / 值对象 / 枚举、端口接口、方法签名、查询与命令数据结构。
**本文不写**：实现类、Graph SDK 调用细节、SQLite SQL、Tauri command 函数体、Adapter 类体。

约定：

- Domain 使用数据结构与纯函数；Application 用例与 Infrastructure Repository 使用模块函数；源码落在 `src-tauri/src/`。
- 前端入站以 **TypeScript `interface`** 描述；Port 落在前端 `ports/`，Adapter 另文实现。
- 账户内业务入口均带 `account_id`；登录与列出账户是账户建立前的例外。
- ID、时间戳、邮箱规范化规则与数据模型一致。
- 代码注释与文档说明一律使用**简体中文**（标识符保持英文）。
- `Result` / `Promise` 错误统一映射为可序列化 `AppError`（见文末）。

依赖方向：`前端 Port` → RS 桥（`commands/`）→ `application` → `infrastructure`（具体模块函数） / `domain`（纯规则）；`infrastructure` → `domain`（出站实现不在本文）。

---

## 0. 目录落点（强制）

规格中的类型 / 接口按下表落文件；**不设** Rust 侧 `interfaces/` 入站层。

### 0.1 Rust（`src-tauri/src/`）

```text
src-tauri/src/
  lib.rs
  domain/
    entities/                      # 纯领域实体
    value_objects/                 # ID、地址、枚举
    services/                      # 聚合、邮件归属、self 地址纯函数
    error.rs
  application/
    dto/                           # 命令、查询、远端快照、AggregateClaim
    use_cases/                     # 模块函数；接收 &SqlitePool
    error.rs
  infrastructure/
    repository/                    # accounts、contacts、messages、groups 等模块
      rows.rs                      # SQLite 行到领域实体映射
    graph/                         # mail、contacts、auth；仅此处实现 Graph / OAuth
  commands/                        # Tauri IPC 薄胶水
```

Application 按当前项目约定直接依赖具体 Infrastructure 模块。取消 Repository / TransactionManager / Policy 对象注入，不建立同名 trait 与实现类；这替代此前严格依赖倒置的目录设计。Domain 仍不依赖 Application、数据库、Graph 或 Tauri。前端 Port → Adapter 的接口边界保持不变。

### 0.2 前端（建议 `src/`；若 monorepo 可放 `packages/app/src/`，相对结构不变）

```
src/
  ports/                           # 入站接口（仅 interface，无实现）
    accountPort.ts
    syncPort.ts
    contactPort.ts
    conversationPort.ts
    messagePort.ts
    groupPort.ts
  domain/                          # 与 Rust 同构的 TS 类型（仅数据结构）
    types.ts                       # 值对象别名、枚举字面量
    entities.ts                    # Account、Contact、Message…
    dto.ts                         # CursorPage、*Query、*Command、读模型
    errors.ts                      # AppError
  adapters/                        # （本文不写）Tauri*Adapter 实现类
  features/ | ui/                  # （本文不写）只依赖 ports，不直接 invoke
```

下文每个代码块上方用 `路径：` 标明落点；同一文件内可含多个紧密相关类型。

### 0.3 IPC 类型与序列化

Rust 字段及 IPC DTO 的 JSON 字段统一 `snake_case`，TS 数据属性同名；TS Port 方法名保持 camelCase。枚举使用 `#[serde(rename_all = "snake_case")]`，ID newtype 使用透明字符串序列化。`Option<T>` 固定输出 `null`，不省略字段；`Some("")` 保留为空字符串。时间戳为 Unix 毫秒整数，传输整数必须落在 JS 安全整数范围内；字节数组映射为 `number[]`。

Domain 保持纯 Rust；序列化由 Application/IPC DTO 映射完成，不要求领域实体直接 derive serde。以 Rust 可序列化 DTO 为单一来源生成 TS 类型，前端 `domain/` 是传输类型的导出位置；CI 检查生成差异，避免手工双写漂移。契约验证覆盖枚举、null、空正文、ID 与时间戳。错误经 Application 统一映射后由 Adapter 转为带 AppError 数据的 Promise rejection。

---

## 1. 共享值对象与枚举

路径：`src-tauri/src/domain/value_objects/`

```rust
/// Unix 毫秒时间戳
pub type TimestampMs = i64;

/// 规范化邮箱：trim + lowercase
pub struct EmailAddress(String);

/// 账户 ID
pub struct AccountId(String);
/// 联系人本地 ID
pub struct ContactId(String);
/// 邮件本地 ID（稳定；与 graph_id 分离）
pub struct MessageId(String);
/// 业务群组 ID
pub struct GroupId(String);
/// 会话列表缓存 ID
pub struct ConversationId(String);
/// Graph conversationId / 线程 ID
pub struct ThreadId(String);
/// Graph ImmutableId
pub struct GraphImmutableId(String);
/// Graph 邮件文件夹 ID（仅同步投影，非导航主轴）
pub struct FolderId(String);

/// 账户级 / 文件夹级同步状态机
pub enum SyncStatus {
    /// 空闲
    Idle,
    /// 进行中
    Running,
    /// 失败
    Error,
}

/// 单封邮件相对远端的同步状态
pub enum MessageSyncStatus {
    /// 已与远端对齐
    Synced,
    /// 仅本地变更，待推送
    PendingLocal,
    /// 远端已删（软删）；不参与列表/未读
    DeletedRemote,
}

/// 邮件正文类型
pub enum BodyContentType {
    Text,
    Html,
}

/// 重要度
pub enum Importance {
    Low,
    Normal,
    High,
}

/// 收件人角色（对应 message_recipients.role）
pub enum RecipientRole {
    To,
    Cc,
    Bcc,
    ReplyTo,
}

/// 群组诞生方式（不是线程归组的第二套规则）
pub enum GroupSource {
    /// 聚合自动创建
    Auto,
    /// 用户空手创建
    Manual,
}

/// 会话种类
pub enum ConversationKind {
    /// 联系人一对一投影
    Contact,
    /// 群组会话
    Group,
}

/// 会话聚合任务状态
pub enum AggregateJobStatus {
    /// 首次待处理
    Pending,
    /// 持有租约处理中
    Processing,
    /// 已完成
    Completed,
    /// 失败后待重试
    Retry,
    /// 有限重试耗尽或永久错误，等待人工处理或新数据
    Failed,
}

/// 会话内消息分页方向
pub enum PageDirection {
    /// 更旧（时间倒序「加载更多」）
    Older,
    /// 更新
    Newer,
}
```

路径：`src/domain/types.ts`（前端镜像，同名同语义）

```ts
/** Unix 毫秒时间戳 */
export type TimestampMs = number;
/** 规范化邮箱 */
export type EmailAddress = string;
export type AccountId = string;
export type ContactId = string;
export type MessageId = string;
export type GroupId = string;
export type ConversationId = string;
export type ThreadId = string;
export type GraphImmutableId = string;
export type FolderId = string;

/** 账户 / 文件夹同步状态 */
export type SyncStatus = 'idle' | 'running' | 'error';
/** 单封邮件同步状态 */
export type MessageSyncStatus = 'synced' | 'pending_local' | 'deleted_remote';
export type BodyContentType = 'text' | 'html';
export type Importance = 'low' | 'normal' | 'high';
export type RecipientRole = 'to' | 'cc' | 'bcc' | 'reply_to';
/** 群组诞生方式 */
export type GroupSource = 'auto' | 'manual';
export type ConversationKind = 'contact' | 'group';
export type AggregateJobStatus = 'pending' | 'processing' | 'completed' | 'retry' | 'failed';
export type AggregationOutcome = 'completed' | 'partial' | 'deferred' | 'skipped';
export type PageDirection = 'older' | 'newer';
```

---

## 2. 领域实体

纯数据结构；不变量由领域服务 / 用例在写入路径维护。基线字段来自 `data-model.md`；本次修订的持久化差异见 §12，迁移前不得假定两文已完全一致。

### 2.1 Account

路径：`src-tauri/src/domain/entities/account.rs`

```rust
/// Outlook 账户（租户根）
pub struct Account {
    pub id: AccountId,
    /// 主邮箱（规范化）
    pub email: EmailAddress,
    pub display_name: String,
    /// self 额外身份；与 email 组成排除集合
    pub aliases: Vec<EmailAddress>,
    pub tenant_id: Option<String>,
    pub home_account_id: Option<String>,
    /// 联系人 Delta 游标
    pub contact_delta_link: Option<String>,
    /// 最近一次账户邮件同步轮次全部成功结束时间
    pub mail_synced_at: Option<TimestampMs>,
    /// 最近一次联系人同步成功结束时间
    pub contact_synced_at: Option<TimestampMs>,
    pub mail_sync_status: SyncStatus,
    /// 最近一次失败原因；成功时清空
    pub mail_sync_error: Option<String>,
    /// 群组消息是否同时出现在联系人会话投影（默认否）
    pub show_group_messages_in_contact_conversation: bool,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// self 排除集合 = {email} ∪ aliases
pub struct SelfAddressSet {
    pub addresses: Vec<EmailAddress>,
}
```

### 2.2 MailFolderSyncState

路径：`src-tauri/src/domain/entities/sync_state.rs`

```rust
/// 按账户 + 文件夹保存的邮件增量游标（非文件夹导航模型）
pub struct MailFolderSyncState {
    pub account_id: AccountId,
    pub folder_id: FolderId,
    /// 最近完成轮次的增量游标
    pub delta_link: Option<String>,
    /// 当前未完成轮次的分页续传位置
    pub next_link: Option<String>,
    pub synced_at: Option<TimestampMs>,
    pub sync_status: SyncStatus,
    pub last_error: Option<String>,
    pub updated_at: TimestampMs,
}
```

### 2.3 Contact

路径：`src-tauri/src/domain/entities/contact.rs`

```rust
/// 通讯录本地缓存
pub struct Contact {
    pub id: ContactId,
    pub account_id: AccountId,
    /// 可空：允许仅按地址建的占位联系人
    pub graph_id: Option<String>,
    pub display_name: String,
    /// 主地址（账户内唯一）
    pub normalized_address: EmailAddress,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}
```

### 2.4 Message / MessageRecipient

路径：`src-tauri/src/domain/entities/message.rs`

```rust
/// 邮件真相（Graph 同步落库）；会话归属不改写本实体
pub struct Message {
    pub id: MessageId,
    pub account_id: AccountId,
    /// Graph ImmutableId；本地草稿可空
    pub graph_id: Option<GraphImmutableId>,
    /// Graph conversationId；联系人投影不要求非空，线程归组要求非空
    pub thread_id: Option<ThreadId>,
    pub graph_change_key: Option<String>,
    /// RFC Message-ID
    pub internet_message_id: Option<String>,
    pub conversation_index: Option<Vec<u8>>,
    pub subject: String,
    pub body_preview: String,
    pub body_content_type: BodyContentType,
    /// None=未加载；Some(空串)=已加载且正文为空
    pub body_content: Option<String>,
    pub from_address: Option<EmailAddress>,
    pub from_name: String,
    pub received_at: Option<TimestampMs>,
    pub sent_at: Option<TimestampMs>,
    pub created_at_graph: Option<TimestampMs>,
    pub is_read: bool,
    pub is_draft: bool,
    /// 是否本账户发出（结合 self 集合判定后落库）
    pub is_outbound: bool,
    pub has_attachments: bool,
    pub importance: Importance,
    /// 当前位置文件夹；非主导航
    pub folder_id: Option<FolderId>,
    pub parent_folder_name: Option<String>,
    pub sync_status: MessageSyncStatus,
    pub local_updated_at: TimestampMs,
    pub remote_updated_at: Option<TimestampMs>,
}

/// 收件人拆行
pub struct MessageRecipient {
    pub account_id: AccountId,
    pub message_id: MessageId,
    pub role: RecipientRole,
    /// 同角色内顺序
    pub seq: i32,
    pub normalized_address: EmailAddress,
    pub display_name: String,
}

/// 邮件 + 收件人读写聚合
pub struct MessageWithRecipients {
    pub message: Message,
    pub recipients: Vec<MessageRecipient>,
}
```

### 2.5 Group / GroupThread / GroupParticipant

路径：`src-tauri/src/domain/entities/group.rs`

```rust
/// 业务群组（自动 / 手动）
pub struct Group {
    pub id: GroupId,
    pub account_id: AccountId,
    pub name: String,
    /// 名称是否用户手动指定
    pub name_is_manual: bool,
    pub source: GroupSource,
    /// 发信成员是否手动锁定
    pub send_members_is_manual: bool,
    /// 被吸收时指向存活根群组
    pub merged_into_id: Option<GroupId>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// 线程 → 群组主归属（账户内每个 thread 最多一行）
pub struct GroupThread {
    pub account_id: AccountId,
    pub group_id: GroupId,
    pub thread_id: ThreadId,
    pub linked_at: TimestampMs,
}

/// 群组成员投影（观测 + 发信）
pub struct GroupParticipant {
    pub account_id: AccountId,
    pub group_id: GroupId,
    pub normalized_address: EmailAddress,
    pub display_name: String,
    /// 是否由邮件往来观测得到
    pub is_observed: bool,
    /// 是否发信成员
    pub is_send_member: bool,
    pub first_seen_at: TimestampMs,
    pub last_seen_at: TimestampMs,
}
```

### 2.6 Conversation / ConversationMessageLink

路径：`src-tauri/src/domain/entities/conversation.rs`

```rust
/// 会话列表缓存；归属真相在 group_thread + conversation_messages
pub struct Conversation {
    pub id: ConversationId,
    pub account_id: AccountId,
    pub kind: ConversationKind,
    /// kind=contact 时非空
    pub normalized_address: Option<EmailAddress>,
    /// 可空；删联系人时清空引用，保留会话
    pub contact_id: Option<ContactId>,
    /// kind=group 时非空
    pub group_id: Option<GroupId>,
    pub last_message_at: Option<TimestampMs>,
    /// 须指向当前附着的非软删消息
    pub last_message_id: Option<MessageId>,
    /// 仅统计当前会话内非软删且未读
    pub unread_count: i64,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

/// 消息 ↔ 会话多对多附着
pub struct ConversationMessageLink {
    pub account_id: AccountId,
    pub conversation_id: ConversationId,
    pub message_id: MessageId,
    pub attached_at: TimestampMs,
}
```

### 2.7 ConversationAggregateJob

路径：`src-tauri/src/domain/entities/aggregate_job.rs`

```rust
/// 聚合任务（失败恢复 / 租约）
pub struct ConversationAggregateJob {
    pub account_id: AccountId,
    pub message_id: MessageId,
    /// 每次有效消息变更重新入队时单调递增
    pub generation: i64,
    pub outcome: Option<AggregationOutcome>,
    pub status: AggregateJobStatus,
    pub attempts: i32,
    pub last_error: Option<String>,
    /// 最早可处理时间
    pub available_at: TimestampMs,
    pub updated_at: TimestampMs,
    /// processing 时非空
    pub lease_token: Option<String>,
    pub lease_expires_at: Option<TimestampMs>,
}
```

前端实体路径：`src/domain/entities.ts`（字段同构，注释同样用中文）。

---

## 3. 查询 / 命令数据结构

路径：`src-tauri/src/application/dto/` 与 `src/domain/dto.ts`

### 3.1 分页与列表查询

```rust
/// 游标分页结果
pub struct CursorPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// 会话列表查询
pub struct ConversationListQuery {
    pub account_id: AccountId,
    pub cursor: Option<String>,
    pub limit: u32,
    /// 可选：仅 contact / 仅 group
    pub kind: Option<ConversationKind>,
}

/// 会话内消息查询
pub struct ConversationMessagesQuery {
    pub account_id: AccountId,
    pub conversation_id: ConversationId,
    pub cursor: Option<String>,
    pub limit: u32,
    /// 默认 Older
    pub direction: PageDirection,
}

/// 联系人列表查询
pub struct ContactListQuery {
    pub account_id: AccountId,
    pub cursor: Option<String>,
    pub limit: u32,
    /// 显示名 / 地址前缀
    pub search: Option<String>,
}

/// 群组列表查询
pub struct GroupListQuery {
    pub account_id: AccountId,
    pub cursor: Option<String>,
    pub limit: u32,
    /// 默认 false：只返回根群组（merged_into_id 为空）
    pub include_merged: bool,
}
```

### 3.2 会话列表读模型

```rust
/// 会话列表项（含预览派生）
pub struct ConversationListItem {
    pub conversation: Conversation,
    /// 列表预览；软删消息不出现
    pub last_message_preview: Option<MessagePreview>,
    pub contact_display_name: Option<String>,
    pub group_name: Option<String>,
}

/// 列表用消息摘要
pub struct MessagePreview {
    pub id: MessageId,
    pub subject: String,
    pub body_preview: String,
    pub from_address: Option<EmailAddress>,
    pub from_name: String,
    pub received_at: Option<TimestampMs>,
    pub sent_at: Option<TimestampMs>,
    pub is_read: bool,
    pub is_outbound: bool,
    pub has_attachments: bool,
    pub importance: Importance,
}
```

### 3.3 写入命令

```rust
/// 登录后落库 / 刷新账户资料
pub struct UpsertAccountCommand {
    pub id: AccountId,
    pub email: EmailAddress,
    pub display_name: String,
    pub aliases: Vec<EmailAddress>,
    pub tenant_id: Option<String>,
    pub home_account_id: Option<String>,
}

/// 更新账户显示配置
pub struct UpdateAccountDisplaySettingsCommand {
    pub account_id: AccountId,
    pub show_group_messages_in_contact_conversation: bool,
}

/// 批量标记已读 / 未读
pub struct MarkMessagesReadCommand {
    pub account_id: AccountId,
    pub message_ids: Vec<MessageId>,
    pub is_read: bool,
}

/// 用户空手创建群组
pub struct CreateManualGroupCommand {
    pub account_id: AccountId,
    pub name: String,
    pub send_member_addresses: Vec<EmailAddress>,
}

/// 重命名群组（同时标记 name_is_manual）
pub struct RenameGroupCommand {
    pub account_id: AccountId,
    pub group_id: GroupId,
    pub name: String,
}

/// 合并群组：source 吸收进 target（存活根）
pub struct MergeGroupsCommand {
    pub account_id: AccountId,
    pub source_group_id: GroupId,
    pub target_group_id: GroupId,
}

/// 把线程挪到另一群组（同事务迁移附着与两侧投影）
pub struct MoveThreadToGroupCommand {
    pub account_id: AccountId,
    pub thread_id: ThreadId,
    pub target_group_id: GroupId,
}

/// 更新发信成员集合
pub struct UpdateGroupSendMembersCommand {
    pub account_id: AccountId,
    pub group_id: GroupId,
    pub send_member_addresses: Vec<EmailAddress>,
    /// true：锁定为手动，观测不再覆盖发信集合
    pub is_manual: bool,
}

/// 确保存在联系人会话（可顺带占位联系人）
pub struct EnsureContactConversationCommand {
    pub account_id: AccountId,
    pub normalized_address: EmailAddress,
    pub display_name: Option<String>,
}
```

### 3.4 同步相关

```rust
/// 邮件同步进度读模型
pub struct MailSyncProgress {
    pub account_id: AccountId,
    pub mail_sync_status: SyncStatus,
    pub mail_sync_error: Option<String>,
    pub mail_synced_at: Option<TimestampMs>,
    pub folders: Vec<MailFolderSyncState>,
}

/// 联系人同步进度读模型
pub struct ContactSyncProgress {
    pub account_id: AccountId,
    pub contact_synced_at: Option<TimestampMs>,
    pub contact_delta_link: Option<String>,
}

/// 触发邮件同步
pub struct TriggerMailSyncCommand {
    pub account_id: AccountId,
    /// 空则使用当前跟踪文件夹集合
    pub folder_ids: Option<Vec<FolderId>>,
}

/// 触发联系人同步
pub struct TriggerContactSyncCommand {
    pub account_id: AccountId,
}
```

---

## 4. Infrastructure 模块函数

Repository 位于 `src-tauri/src/infrastructure/repository/`。Application 直接导入模块，以 `SqlitePool` 获取连接或开启事务；Repository 只借用 `&mut SqliteConnection`，不获取新连接、不提交事务。外部 Graph/OAuth 也通过 Infrastructure 模块函数调用，Graph 流量仍只使用 graph-rs-sdk。

以下签名是当前骨架契约，不表示全部功能已实现；尚未实现的方法返回明确错误，不能当作成功空结果。

### 4.1 `repository::accounts`

```rust
pub async fn get(
    conn: &mut SqliteConnection,
    id: &AccountId,
) -> Result<Option<Account>, PortError>;

pub async fn get_by_email(
    conn: &mut SqliteConnection,
    email: &EmailAddress,
) -> Result<Option<Account>, PortError>;

pub async fn list(conn: &mut SqliteConnection) -> Result<Vec<Account>, PortError>;

pub async fn upsert(conn: &mut SqliteConnection, account: &Account) -> Result<(), PortError>;

pub async fn update_mail_sync_meta(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    status: SyncStatus,
    error: Option<&str>,
    mail_synced_at: Option<TimestampMs>,
) -> Result<(), PortError>;

pub async fn update_contact_sync_meta(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    contact_delta_link: Option<&str>,
    contact_synced_at: Option<TimestampMs>,
) -> Result<(), PortError>;

pub async fn update_display_settings(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    show_group_messages_in_contact_conversation: bool,
) -> Result<(), PortError>;

pub async fn delete(conn: &mut SqliteConnection, id: &AccountId) -> Result<(), PortError>;
```

### 4.2 `repository::folder_states`

```rust
pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<Option<MailFolderSyncState>, PortError>;

pub async fn list_by_account(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
) -> Result<Vec<MailFolderSyncState>, PortError>;

pub async fn upsert(
    conn: &mut SqliteConnection,
    state: &MailFolderSyncState,
) -> Result<(), PortError>;

pub async fn reset_cursors(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<(), PortError>;

pub async fn delete(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    folder_id: &FolderId,
) -> Result<(), PortError>;
```

### 4.3 `repository::contacts`

```rust
pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &ContactId,
) -> Result<Option<Contact>, PortError>;

pub async fn get_by_address(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    address: &EmailAddress,
) -> Result<Option<Contact>, PortError>;

pub async fn list(
    conn: &mut SqliteConnection,
    query: &ContactListQuery,
) -> Result<CursorPage<Contact>, PortError>;

pub async fn upsert(conn: &mut SqliteConnection, contact: &Contact) -> Result<(), PortError>;

pub async fn delete(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &ContactId,
) -> Result<(), PortError>;
```

### 4.4 `repository::messages`

```rust
pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &MessageId,
) -> Result<Option<MessageWithRecipients>, PortError>;

pub async fn get_by_graph_id(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    graph_id: &GraphImmutableId,
) -> Result<Option<MessageWithRecipients>, PortError>;

pub async fn upsert_with_recipients(
    conn: &mut SqliteConnection,
    message: &MessageWithRecipients,
) -> Result<(), PortError>;

pub async fn mark_read(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_ids: &[MessageId],
    is_read: bool,
) -> Result<Vec<MessageId>, PortError>;

pub async fn mark_deleted_remote(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_ids: &[MessageId],
) -> Result<(), PortError>;

pub async fn delete_physical(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_ids: &[MessageId],
) -> Result<(), PortError>;

pub async fn list_by_thread(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    thread_id: &ThreadId,
) -> Result<Vec<Message>, PortError>;
```

### 4.5 `repository::groups`

```rust
pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &GroupId,
) -> Result<Option<Group>, PortError>;

pub async fn resolve_root(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &GroupId,
) -> Result<Option<Group>, PortError>;

pub async fn list(
    conn: &mut SqliteConnection,
    query: &GroupListQuery,
) -> Result<CursorPage<Group>, PortError>;

pub async fn insert(conn: &mut SqliteConnection, group: &Group) -> Result<(), PortError>;

pub async fn update(conn: &mut SqliteConnection, group: &Group) -> Result<(), PortError>;

pub async fn list_participants(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Vec<GroupParticipant>, PortError>;

pub async fn upsert_participants(
    conn: &mut SqliteConnection,
    participants: &[GroupParticipant],
) -> Result<(), PortError>;

pub async fn replace_send_members(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
    members: &[GroupParticipant],
    send_members_is_manual: bool,
) -> Result<(), PortError>;

pub async fn get_thread_link(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    thread_id: &ThreadId,
) -> Result<Option<GroupThread>, PortError>;

pub async fn upsert_thread_link(
    conn: &mut SqliteConnection,
    link: &GroupThread,
) -> Result<(), PortError>;

pub async fn list_threads_by_group(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Vec<GroupThread>, PortError>;
```

### 4.6 `repository::conversations`

```rust
pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    id: &ConversationId,
) -> Result<Option<Conversation>, PortError>;

pub async fn get_contact_conversation(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    address: &EmailAddress,
) -> Result<Option<Conversation>, PortError>;

pub async fn get_group_conversation(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Option<Conversation>, PortError>;

pub async fn list(
    conn: &mut SqliteConnection,
    query: &ConversationListQuery,
) -> Result<CursorPage<ConversationListItem>, PortError>;

pub async fn upsert(
    conn: &mut SqliteConnection,
    conversation: &Conversation,
) -> Result<(), PortError>;

pub async fn clear_contact_id_refs(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    contact_id: &ContactId,
) -> Result<(), PortError>;

pub async fn attach_messages(
    conn: &mut SqliteConnection,
    links: &[ConversationMessageLink],
) -> Result<(), PortError>;

pub async fn detach_messages(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    conversation_id: &ConversationId,
    message_ids: &[MessageId],
) -> Result<(), PortError>;

pub async fn list_message_ids(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    conversation_id: &ConversationId,
) -> Result<Vec<MessageId>, PortError>;

pub async fn list_messages(
    conn: &mut SqliteConnection,
    query: &ConversationMessagesQuery,
) -> Result<CursorPage<MessageWithRecipients>, PortError>;

pub async fn recompute_projection(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    conversation_id: &ConversationId,
) -> Result<Conversation, PortError>;

pub async fn find_conversation_ids_by_message(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<ConversationId>, PortError>;
```

### 4.7 `repository::jobs`

```rust
pub async fn enqueue_or_reset(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    available_at: TimestampMs,
) -> Result<(), PortError>;

pub async fn claim_batch(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
    lease_token: &str,
    lease_expires_at: TimestampMs,
    limit: u32,
) -> Result<Vec<ConversationAggregateJob>, PortError>;

pub async fn complete(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    lease_token: &str,
    generation: i64,
    outcome: AggregationOutcome,
) -> Result<(), PortError>;

pub async fn fail_for_retry(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    lease_token: &str,
    generation: i64,
    error: &str,
    available_at: TimestampMs,
) -> Result<(), PortError>;

pub async fn fail_permanently(
    conn: &mut SqliteConnection,
    claim: &AggregateClaim,
    error: &str,
) -> Result<(), PortError>;

pub async fn release_expired_leases(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
) -> Result<u32, PortError>;
```

### 4.8 `repository::mutations`

```rust
pub async fn put_read(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
    desired: bool,
) -> Result<(), PortError>;

pub async fn get(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<PendingMessageMutation>, PortError>;

pub async fn list_due(
    conn: &mut SqliteConnection,
    account_id: &AccountId,
    now: TimestampMs,
    limit: u32,
) -> Result<Vec<PendingMessageMutation>, PortError>;

pub async fn acknowledge(
    conn: &mut SqliteConnection,
    mutation: &PendingMessageMutation,
) -> Result<(), PortError>;

pub async fn record_failure(
    conn: &mut SqliteConnection,
    mutation: &PendingMessageMutation,
    error: &str,
    available_at: TimestampMs,
) -> Result<(), PortError>;
```

### 4.9 具体事务边界

```rust
// Application 用例：同一事务连接传给不同 Repository 模块。
let mut tx = db.begin().await?;
contacts::upsert(&mut tx, &contact).await?;
groups::insert(&mut tx, &group).await?;
tx.commit().await?;
```

写操作由 Application 显式提交；失败时返回错误并释放事务，或显式 rollback。所有相关 Repository 函数必须使用调用者传入的同一连接；不能重新从池取连接后自行提交。读查询可直接使用池连接。Graph/OAuth 网络请求在写事务外完成。

原子范围包括：Delta 页数据 / 收件人 / 核对记录 / 聚合任务与游标；聚合计划执行 / 附着 / 投影与任务完成；线程迁移 / 群组合并及两侧投影；已读修改 / 待提交变更与关联会话未读数；联系人引用清空与删除。

聚合执行前在事务内检查账户、消息、generation、token、租约及有效期，再读取上下文并调用 `decide_aggregation`。旧任务校验失败须回滚所有写入；不能只保护 complete 而允许过期投影提交。重新入队递增 generation 并使旧租约失效，重复处理不能累计附着或计数。SQLite 并发写采用立即写事务或等价串行策略；普通 begin 示例不能替代并发校验。领取任务可使用短事务，每条任务的读取、决策、写入和完成必须保持一致快照。

Graph/OAuth 模块的远端数据结构位于 `application/dto/remote_models.rs`，不向 Domain 注入技术对象；未实现的网络能力保留显式错误。

### 4.10 本地修改与远端冲突

路径：`src-tauri/src/application/dto/pending_message_mutation_repository.rs`

```rust
pub struct PendingMessageMutation {
    pub account_id: AccountId,
    pub message_id: MessageId,
    /// V1 仅支持 is_read；将来按字段扩展
    pub desired_is_read: bool,
    pub version: i64,
    pub attempts: i32,
    pub available_at: TimestampMs,
    pub last_error: Option<String>,
}

// 待提交变更读写函数见 repository::mutations；数据结构位于 application/dto。
```

`MessageSyncStatus` 仅是兼容摘要，不能充当字段冲突依据。远端存活状态与待提交操作分别持久化：软删优先显示 `DeletedRemote`，否则存在待提交操作为 `PendingLocal`，其余为 `Synced`。应用层远端合并必须保留仍待提交的 `is_read`，其他字段正常更新；`upsert_with_recipients` 只接收合并后的本地实体。

每个账户、消息的远端写请求串行发送。PATCH 成功后按版本确认，在读回远端当前值之前保留对应字段保护；版本已更新则继续推送最新值。确认后，已在途的旧 Delta 页必须作废重取，待处理变更亦不得直接覆盖该字段：Delta 的已读值仅作刷新信号，以串行按 ID 读取的当前值消除乱序回退。网络失败保留操作并退避，权限错误保留失败原因供用户重试。确认远端删除后终止该消息操作并保留可诊断原因，不复活消息；新鲜的远端存活查询确认后才允许恢复。

### 4.11 远端模型与文件夹移出判定

路径：`src-tauri/src/application/dto/remote_models.rs`

`RemoteMessage` 是远端快照：包含 `graph_id`、可空 `thread_id`、change key、远端时间、主题、正文预览、可空正文、发送者、收件人、已读 / 草稿 / 附件 / 重要度与当前位置；不含本地 MessageId、account_id、is_outbound、sync_status 或本地时间。收件人仅含角色、顺序、地址、显示名，不含本地外键。Delta 未提供的字段使用 `RemoteField<T> { Unchanged, Value(T) }` 的枚举形式（其中 Value 对可空字段携带 Option），区分“未返回”与“明确清空”。用例合并已有记录，为新记录生成稳定 ID，字段不足时在事务外补齐。

`RemoteContact` 仅含 graph_id、显示名、规范化主地址；`RemoteAccount` 仅含远端账户身份、邮箱、显示名、别名、tenant/home_account_id，不包含本地同步与显示配置。用例负责本地身份和默认值，不允许登录刷新覆盖用户配置。远端账户与本地 AccountId 的映射由 Auth 基础设施在账户建立后绑定。

```rust
pub enum RemoteMessageLocation {
    Present { folder_id: FolderId },
    NotFound,
}
```

Remove 继承请求的账户和源文件夹，只是移出信号。先与页游标原子写入待核对记录，再在事务外调用 `locate_message`；明确仍存活时仅改位置，保留本地 ID 与附着（即使目的文件夹暂未跟踪，也继续按 ID 核对）；只有权威不存在结果才软删。鉴权失败、超时及限流不得当作不存在。

核对记录至少持久化账户、ImmutableId、源文件夹、单调修订版本、重试时间与原因。每次该消息的 Delta upsert/remove 都增加本地核对版本；网络响应只在版本仍匹配的事务内生效，否则丢弃并重新核对。不同文件夹游标没有可比较的全局顺序，change key 也不作为排序数字；跨文件夹乱序时通过串行当前状态查询收敛，旧源文件夹 Remove 不得删除已移动消息。

### 4.12 无 ThreadId、终态与重归组

无 ThreadId 时先执行可确定的联系人投影，输出 `Partial`，不猜测群组。仅 self 或无有效对端且无可执行动作时输出 `Skipped`。两者都是本代 `Completed` 任务的终态，记录 outcome / 原因，不因缺 ThreadId 自动循环重试；后续字段补齐触发新 generation，重算并替换旧附着。

只有需要主动补取的暂时性字段缺失或可恢复故障才为 `Deferred`，进入 `Retry`；每代最多领取 5 次，指数退避，耗尽或永久错误为 `Failed` 并清空租约。新数据或人工重试必须建立新代。Failed 转移需增加仓储 `fail_permanently(claim, error)` 方法，和重试一样校验有效租约。Worker 可调用应用模块函数 `aggregate_message(db, claim)`（待实现） 执行 §4.9 流程；观测成员重算同样由 Application 读取当前有效消息、调用纯函数并写入，不再由 Domain 访问仓储。

V1 一个账户的 Thread 至多一个主 Group。已有 GroupThread（解析到根）优先，参与人变化不自动改组；错误归属通过显式线程迁移纠正。迁移 / 合并后重算线程全量消息附着，自动聚合不能撤销该归属；配置变化须将账户受影响消息重新入队，新计划同时增删联系人与群组附着。

## 5. Domain 纯规则函数

路径：`src-tauri/src/domain/services/`

固定业务规则使用普通函数，不定义仅有一个实现的 Service / Policy trait，也不持有 Arc。规则只接受领域快照并返回数据，不访问仓储、事务或后台任务。

```rust
pub fn resolve_self_addresses(account: &Account) -> SelfAddressSet;
pub fn is_outbound(message: &Message, self_set: &SelfAddressSet) -> bool;
pub fn peer_addresses_for_contact_projection(
    message: &MessageWithRecipients, self_set: &SelfAddressSet,
) -> Vec<EmailAddress>;
pub fn observed_participants(
    message: &MessageWithRecipients, self_set: &SelfAddressSet,
) -> Vec<(EmailAddress, String)>;
pub fn decide_aggregation(context: &AggregationContext) -> Result<AggregationPlan, DomainError>;
```

```rust
/// 聚合决策上下文
#[derive(Debug, Clone)]
pub struct AggregationContext {
    pub message: MessageWithRecipients,
    pub self_set: SelfAddressSet,
    /// 已解析到存活根群组；现有归属优先于自动推断
    pub thread_group: Option<GroupId>,
    pub show_group_messages_in_contact_conversation: bool,
}

/// 群组目标
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupTarget {
    Existing(GroupId),
    Create { members: Vec<EmailAddress> },
}

/// 期望状态；Application 对比旧附着执行增删，避免重复累计
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregationPlan {
    pub contact_addresses: Vec<EmailAddress>,
    pub group_target: Option<GroupTarget>,
    pub observed_participants: Vec<(EmailAddress, String)>,
    pub outcome: AggregationOutcome,
}
```

## 6. Application 用例模块函数

路径：`src-tauri/src/application/use_cases/`

用例接收具体 `&SqlitePool` 与业务参数，直接调用 Infrastructure 模块，领域计算调用纯函数。不注入 Repository、TransactionManager 或 Policy trait，也不以依赖上下文对象重新包装这些接口。

以下仅列签名；现有业务骨架尚未全部实现。

### 6.1 `account_use_case`

```rust
pub async fn list_accounts(db: &SqlitePool) -> Result<Vec<Account>, AppError>;

pub async fn get_account(db: &SqlitePool, account_id: &AccountId) -> Result<Account, AppError>;

pub async fn sign_in(db: &SqlitePool) -> Result<Account, AppError>;

pub async fn sign_out(db: &SqlitePool, account_id: &AccountId) -> Result<(), AppError>;

pub async fn update_display_settings(
    db: &SqlitePool,
    cmd: UpdateAccountDisplaySettingsCommand,
) -> Result<Account, AppError>;
```

### 6.2 `sync_use_case`

```rust
pub async fn trigger_mail_sync(
    db: &SqlitePool,
    cmd: TriggerMailSyncCommand,
) -> Result<(), AppError>;

pub async fn trigger_contact_sync(
    db: &SqlitePool,
    cmd: TriggerContactSyncCommand,
) -> Result<(), AppError>;

pub async fn get_mail_sync_progress(
    db: &SqlitePool,
    account_id: &AccountId,
) -> Result<MailSyncProgress, AppError>;

pub async fn get_contact_sync_progress(
    db: &SqlitePool,
    account_id: &AccountId,
) -> Result<ContactSyncProgress, AppError>;
```

### 6.3 `contact_use_case`

```rust
pub async fn list_contacts(
    db: &SqlitePool,
    query: ContactListQuery,
) -> Result<CursorPage<Contact>, AppError>;

pub async fn get_contact(
    db: &SqlitePool,
    account_id: &AccountId,
    contact_id: &ContactId,
) -> Result<Contact, AppError>;

pub async fn get_contact_by_address(
    db: &SqlitePool,
    account_id: &AccountId,
    address: &EmailAddress,
) -> Result<Option<Contact>, AppError>;
```

### 6.4 `conversation_use_case`

```rust
pub async fn list_conversations(
    db: &SqlitePool,
    query: ConversationListQuery,
) -> Result<CursorPage<ConversationListItem>, AppError>;

pub async fn get_conversation(
    db: &SqlitePool,
    account_id: &AccountId,
    conversation_id: &ConversationId,
) -> Result<ConversationListItem, AppError>;

pub async fn list_messages(
    db: &SqlitePool,
    query: ConversationMessagesQuery,
) -> Result<CursorPage<MessageWithRecipients>, AppError>;

pub async fn ensure_contact_conversation(
    db: &SqlitePool,
    cmd: EnsureContactConversationCommand,
) -> Result<Conversation, AppError>;
```

### 6.5 `message_use_case`

```rust
pub async fn get_message(
    db: &SqlitePool,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<MessageWithRecipients, AppError>;

pub async fn ensure_message_body(
    db: &SqlitePool,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<MessageWithRecipients, AppError>;

pub async fn mark_read(db: &SqlitePool, cmd: MarkMessagesReadCommand) -> Result<(), AppError>;
```

### 6.6 `group_use_case`

```rust
pub async fn list_groups(
    db: &SqlitePool,
    query: GroupListQuery,
) -> Result<CursorPage<Group>, AppError>;

pub async fn get_group(
    db: &SqlitePool,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Group, AppError>;

pub async fn list_participants(
    db: &SqlitePool,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Vec<GroupParticipant>, AppError>;

pub async fn create_manual_group(
    db: &SqlitePool,
    cmd: CreateManualGroupCommand,
) -> Result<Group, AppError>;

pub async fn rename_group(db: &SqlitePool, cmd: RenameGroupCommand) -> Result<Group, AppError>;

pub async fn merge_groups(db: &SqlitePool, cmd: MergeGroupsCommand) -> Result<Group, AppError>;

pub async fn move_thread_to_group(
    db: &SqlitePool,
    cmd: MoveThreadToGroupCommand,
) -> Result<(), AppError>;

pub async fn update_send_members(
    db: &SqlitePool,
    cmd: UpdateGroupSendMembersCommand,
) -> Result<Vec<GroupParticipant>, AppError>;
```

### 6.7 `aggregate_worker_use_case`

```rust
pub async fn process_available_jobs(db: &SqlitePool, limit: u32) -> Result<u32, AppError>;
```

## 7. 前端 Port（入站）

路径：`src/ports/*.ts`
UI / features / stores **只依赖**下列接口。实现类不在本文。

```ts
/** 账户入站端口 */
export interface AccountPort {
  /** 列出已登录账户 */
  listAccounts(): Promise<Account[]>;
  getAccount(accountId: AccountId): Promise<Account>;
  signIn(): Promise<Account>;
  signOut(accountId: AccountId): Promise<void>;
  updateDisplaySettings(
    cmd: UpdateAccountDisplaySettingsCommand,
  ): Promise<Account>;
}

/** 同步入站端口 */
export interface SyncPort {
  triggerMailSync(cmd: TriggerMailSyncCommand): Promise<void>;
  triggerContactSync(cmd: TriggerContactSyncCommand): Promise<void>;
  getMailSyncProgress(accountId: AccountId): Promise<MailSyncProgress>;
  getContactSyncProgress(accountId: AccountId): Promise<ContactSyncProgress>;
}

/** 联系人入站端口 */
export interface ContactPort {
  listContacts(query: ContactListQuery): Promise<CursorPage<Contact>>;
  getContact(accountId: AccountId, contactId: ContactId): Promise<Contact>;
  getContactByAddress(
    accountId: AccountId,
    address: EmailAddress,
  ): Promise<Contact | null>;
}

/** 会话入站端口（主导航） */
export interface ConversationPort {
  listConversations(
    query: ConversationListQuery,
  ): Promise<CursorPage<ConversationListItem>>;
  getConversation(
    accountId: AccountId,
    conversationId: ConversationId,
  ): Promise<ConversationListItem>;
  listMessages(
    query: ConversationMessagesQuery,
  ): Promise<CursorPage<MessageWithRecipients>>;
  ensureContactConversation(
    cmd: EnsureContactConversationCommand,
  ): Promise<Conversation>;
}

/** 邮件入站端口 */
export interface MessagePort {
  getMessage(
    accountId: AccountId,
    messageId: MessageId,
  ): Promise<MessageWithRecipients>;
  /** 按需拉取正文 */
  ensureMessageBody(
    accountId: AccountId,
    messageId: MessageId,
  ): Promise<MessageWithRecipients>;
  markRead(cmd: MarkMessagesReadCommand): Promise<void>;
}

/** 群组入站端口 */
export interface GroupPort {
  listGroups(query: GroupListQuery): Promise<CursorPage<Group>>;
  getGroup(accountId: AccountId, groupId: GroupId): Promise<Group>;
  listParticipants(
    accountId: AccountId,
    groupId: GroupId,
  ): Promise<GroupParticipant[]>;
  createManualGroup(cmd: CreateManualGroupCommand): Promise<Group>;
  renameGroup(cmd: RenameGroupCommand): Promise<Group>;
  mergeGroups(cmd: MergeGroupsCommand): Promise<Group>;
  moveThreadToGroup(cmd: MoveThreadToGroupCommand): Promise<void>;
  updateSendMembers(
    cmd: UpdateGroupSendMembersCommand,
  ): Promise<GroupParticipant[]>;
}
```

---

## 8. 错误结构

DomainError 仅表达业务规则；PortError 暂保留为基础设施技术错误类型，位于 Application；不代表仍存在 trait 端口。Application 将不存在映射为 not_found、输入错误为 validation、规则或版本冲突为 conflict、鉴权为 auth、网络为 network、远端同步故障为 sync、存储故障为 internal，并按故障性质设置 retryable。面向用户的 message 必须脱敏，底层诊断只进本地日志。

路径：`src-tauri/src/domain/error.rs`、`src-tauri/src/application/error.rs`、`src/domain/errors.ts`

```rust
/// 领域层错误
pub enum DomainError {
    /// 实体不存在
    NotFound { entity: &'static str, id: String },
    /// 冲突（合并环、重复根等）
    Conflict { message: String },
    /// 不变量破坏
    InvariantViolation { message: String },

}

/// 应用边界出站错误；不属于 Domain，禁止泄漏 SQL / Token / SDK 错误详情
pub enum PortError {
    Storage { message: String },
    Auth { message: String },
    Network { message: String },
    Remote { message: String, retryable: bool },
    Conflict { message: String },
}

/// 可序列化到前端的应用错误
pub struct AppError {
    pub code: AppErrorCode,
    /// 稳定说明，可供 i18n 映射
    pub message: String,
    pub retryable: bool,
}

pub enum AppErrorCode {
    NotFound,
    Validation,
    Conflict,
    Auth,
    Sync,
    Network,
    Internal,
}
```

```ts
/** 前端可展示错误 */
export interface AppError {
  code:
    | 'not_found'
    | 'validation'
    | 'conflict'
    | 'auth'
    | 'sync'
    | 'network'
    | 'internal';
  message: string;
  retryable: boolean;
}
```

---

## 9. 端口与数据模型 / 目录对照

实体相对于 `src-tauri/src/domain/entities/`，Repository 相对于 `src-tauri/src/infrastructure/repository/`：

| 数据 | 实体 | Repository 模块 |
|---|---|---|
| 账户 | account.rs | accounts.rs |
| 文件夹同步状态 | sync_state.rs | folder_states.rs |
| 联系人 | contact.rs | contacts.rs |
| 邮件与收件人 | message.rs | messages.rs |
| 群组及线程关系 | group.rs | groups.rs |
| 会话与附着 | conversation.rs | conversations.rs |
| 聚合任务 | aggregate_job.rs | jobs.rs |

| 前端能力 | Port 文件 |
|---|---|
| 账户 | `src/ports/accountPort.ts` |
| 同步 | `src/ports/syncPort.ts` |
| 联系人 | `src/ports/contactPort.ts` |
| 会话 | `src/ports/conversationPort.ts` |
| 邮件 | `src/ports/messagePort.ts` |
| 群组 | `src/ports/groupPort.ts` |

归属真相：`group_thread` + `conversation_messages`。列表读 `repository::conversations::list`；详情消息走 `list_messages`。

---

## 10. V1 范围（接口层刻意不做）

- 发信 / 草稿编辑 / 回复 / 转发 Port
- 附件下载与 `message_attachments`
- 文件夹树导航 API
- 跨账户统一会话
- Send-As
- 前端直连 Graph 的任何方法
- `infrastructure/`、`commands/`、`adapters/` 实现类正文

---

## 11. 按业务闭环落地

1. 骨架与契约：只落首条闭环需要的实体、Repository 模块签名、具体事务、错误和 IPC 类型；先确认 §12 的模型迁移。
2. 账户与邮件持久化：OAuth → Rust Graph 模块 → SQLite（消息、任务、游标原子提交）→ 前端 Port 读取；验证稳定本地 ID 和正文懒加载。
3. 联系人投影：self 排除、占位联系人、附着与最新消息 / 未读统计；覆盖无 ThreadId 路径。
4. 群组聚合：自动归组、合并与线程迁移；验证跨仓储回滚、根群组约束及人工归属优先。
5. 同步恢复：补齐断点续传、移出判定、本地操作重放、租约过期与 generation 并发测试。基础幂等与事务规则从阶段二即生效。
6. 后续版本扩展：发信、附件、草稿编辑与回复不纳入本 V1 接口实现。

## 12. 与数据模型及仓库约定的配套差异

本次仅修订本文件。`data-model.md` 的下列旧约定需要在数据库实现前同步修订并编写迁移；不得照旧 DDL 实现本契约：

| 项目 | 必须配套的模型变更 |
|---|---|
| 正文 | `body_content` 改为可空；历史空串无法推断是否加载，按未加载处理并允许重新获取 |
| 无 ThreadId | 取消“入任何会话前必须非空”；仅线程级群组聚合要求非空 |
| 聚合任务 | 增加 `generation`、`outcome`，状态约束增加 `failed`；旧任务迁移为待处理新代 |
| 本地写入 | 增加 §4.10 待提交变更表；唯一键为账户、消息、字段，保存版本、期望值、重试信息 |
| 移出确认 | 增加持久化移出核对记录及修订版本，和文件夹页游标原子提交 |

`AGENTS.md` 中 `src-tauri/domain/` 等路径视为逻辑分层示意；实际 Cargo 未配置自定义入口，使用 `src-tauri/src/`，从 `lib.rs` 声明模块。Application 直接调用 Infrastructure 模块函数，采用当前确认的具体数据库依赖约定，不新增 Rust `interfaces/`。

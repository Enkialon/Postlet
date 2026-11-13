# V1.0.0 接口与实体规格

基于 [`data-model.md`](./data-model.md) 与 [`AGENTS.md`](../../AGENTS.md)。

**本文只定义**：目录落点、实体 / 值对象 / 枚举、端口接口、方法签名、查询与命令数据结构。  
**本文不写**：实现类、Graph SDK 调用细节、SQLite SQL、Tauri command 函数体、Adapter 类体。

约定：

- Domain / Application 以 **Rust trait + 结构体** 描述；源码落在 `src-tauri/`。
- 前端入站以 **TypeScript `interface`** 描述；Port 落在前端 `ports/`，Adapter 另文实现。
- 所有业务入口均带 `account_id`（第一版租户隔离）。
- ID、时间戳、邮箱规范化规则与数据模型一致。
- 代码注释与文档说明一律使用**简体中文**（标识符保持英文）。
- `Result` / `Promise` 错误统一映射为可序列化 `AppError`（见文末）。

依赖方向：`前端 Port` → RS 桥（`commands/`）→ `application` → `domain` ← `infrastructure`（出站实现不在本文）。

---

## 0. 目录落点（强制）

规格中的类型 / 接口按下表落文件；**不设** Rust 侧 `interfaces/` 入站层。

### 0.1 Rust（`src-tauri/`）

```
src-tauri/
  domain/                          # 纯领域：无 graph-rs-sdk / tauri / 文件系统 / 网络
    mod.rs
    error.rs                       # DomainError
    value_objects/                 # 值对象与枚举
      mod.rs
      ids.rs                       # AccountId、MessageId 等
      email.rs                     # EmailAddress、SelfAddressSet
      enums.rs                     # SyncStatus、ConversationKind 等
    entities/                      # 实体（与 data-model 表对应）
      mod.rs
      account.rs
      contact.rs
      message.rs                   # Message、MessageRecipient、MessageWithRecipients
      group.rs                     # Group、GroupThread、GroupParticipant
      conversation.rs              # Conversation、ConversationMessageLink
      sync_state.rs                # MailFolderSyncState
      aggregate_job.rs             # ConversationAggregateJob
    ports/                         # 出站端口（仓储 + Graph/Auth）；仅 trait
      mod.rs
      account_repository.rs
      contact_repository.rs
      message_repository.rs
      group_repository.rs
      conversation_repository.rs
      mail_folder_sync_state_repository.rs
      conversation_aggregate_job_repository.rs
      mail_graph_port.rs
      contact_graph_port.rs
      auth_port.rs
    services/                      # 领域服务接口（trait，无 IO 或仅编排规则）
      mod.rs
      self_address_resolver.rs
      message_attribution.rs
      conversation_aggregation.rs
  application/                     # 用例：编排领域，依赖 ports
    mod.rs
    error.rs                       # AppError / AppErrorCode
    dto/                           # 查询、命令、读模型 DTO
      mod.rs
      page.rs                      # CursorPage、PageDirection
      conversation_dto.rs
      sync_dto.rs
      commands.rs                  # 各类 *Command
    use_cases/                     # 用例接口（trait）；实现类另文
      mod.rs
      account_use_case.rs
      sync_use_case.rs
      contact_use_case.rs
      conversation_use_case.rs
      message_use_case.rs
      group_use_case.rs
      aggregate_worker_use_case.rs
  infrastructure/                  # （本文不写）端口实现：SQLite、graph-rs-sdk、OAuth
  commands/                        # （本文不写）Tauri IPC 薄胶水
```

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

---

## 1. 共享值对象与枚举

路径：`src-tauri/domain/value_objects/`

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
export type AggregateJobStatus = 'pending' | 'processing' | 'completed' | 'retry';
export type PageDirection = 'older' | 'newer';
```

---

## 2. 领域实体

纯数据结构；不变量由领域服务 / 用例在写入路径维护。字段与 `data-model.md` 一一对应。

### 2.1 Account

路径：`src-tauri/domain/entities/account.rs`

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

路径：`src-tauri/domain/entities/sync_state.rs`

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

路径：`src-tauri/domain/entities/contact.rs`

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

路径：`src-tauri/domain/entities/message.rs`

```rust
/// 邮件真相（Graph 同步落库）；会话归属不改写本实体
pub struct Message {
    pub id: MessageId,
    pub account_id: AccountId,
    /// Graph ImmutableId；本地草稿可空
    pub graph_id: Option<GraphImmutableId>,
    /// Graph conversationId；聚合入会话前必须非空
    pub thread_id: Option<ThreadId>,
    pub graph_change_key: Option<String>,
    /// RFC Message-ID
    pub internet_message_id: Option<String>,
    pub conversation_index: Option<Vec<u8>>,
    pub subject: String,
    pub body_preview: String,
    pub body_content_type: BodyContentType,
    /// V1 可先空，按需拉取后回写
    pub body_content: String,
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

路径：`src-tauri/domain/entities/group.rs`

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

路径：`src-tauri/domain/entities/conversation.rs`

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

路径：`src-tauri/domain/entities/aggregate_job.rs`

```rust
/// 聚合任务（失败恢复 / 租约）
pub struct ConversationAggregateJob {
    pub account_id: AccountId,
    pub message_id: MessageId,
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

路径：`src-tauri/application/dto/` 与 `src/domain/dto.ts`

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

## 4. Domain 仓储与出站端口

路径：`src-tauri/domain/ports/`  
由 `infrastructure/` 实现（本文不写实现）。均为异步。

### 4.1 AccountRepository

路径：`…/ports/account_repository.rs`

```rust
/// 账户仓储
#[async_trait]
pub trait AccountRepository: Send + Sync {
    /// 按 ID 获取
    async fn get(&self, id: &AccountId) -> Result<Option<Account>, DomainError>;
    /// 按主邮箱获取
    async fn get_by_email(&self, email: &EmailAddress) -> Result<Option<Account>, DomainError>;
    /// 列出全部账户
    async fn list(&self) -> Result<Vec<Account>, DomainError>;
    /// 插入或更新账户资料
    async fn upsert(&self, account: &Account) -> Result<(), DomainError>;
    /// 更新邮件同步元数据（状态 / 错误 / 成功时间）
    async fn update_mail_sync_meta(
        &self,
        account_id: &AccountId,
        status: SyncStatus,
        error: Option<&str>,
        mail_synced_at: Option<TimestampMs>,
    ) -> Result<(), DomainError>;
    /// 更新联系人同步元数据
    async fn update_contact_sync_meta(
        &self,
        account_id: &AccountId,
        contact_delta_link: Option<&str>,
        contact_synced_at: Option<TimestampMs>,
    ) -> Result<(), DomainError>;
    /// 更新显示配置
    async fn update_display_settings(
        &self,
        cmd: &UpdateAccountDisplaySettingsCommand,
    ) -> Result<(), DomainError>;
    /// 删除账户（级联由库约束处理）
    async fn delete(&self, id: &AccountId) -> Result<(), DomainError>;
}
```

### 4.2 MailFolderSyncStateRepository

路径：`…/ports/mail_folder_sync_state_repository.rs`

```rust
/// 邮件文件夹同步状态仓储
#[async_trait]
pub trait MailFolderSyncStateRepository: Send + Sync {
    async fn get(
        &self,
        account_id: &AccountId,
        folder_id: &FolderId,
    ) -> Result<Option<MailFolderSyncState>, DomainError>;
    /// 列出账户下全部跟踪文件夹状态
    async fn list_by_account(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<MailFolderSyncState>, DomainError>;
    async fn upsert(&self, state: &MailFolderSyncState) -> Result<(), DomainError>;
    /// 清空该文件夹游标（失效重建基线）
    async fn reset_cursors(
        &self,
        account_id: &AccountId,
        folder_id: &FolderId,
    ) -> Result<(), DomainError>;
    async fn delete(
        &self,
        account_id: &AccountId,
        folder_id: &FolderId,
    ) -> Result<(), DomainError>;
}
```

### 4.3 ContactRepository

路径：`…/ports/contact_repository.rs`

```rust
/// 联系人仓储
#[async_trait]
pub trait ContactRepository: Send + Sync {
    async fn get(
        &self,
        account_id: &AccountId,
        id: &ContactId,
    ) -> Result<Option<Contact>, DomainError>;
    async fn get_by_address(
        &self,
        account_id: &AccountId,
        address: &EmailAddress,
    ) -> Result<Option<Contact>, DomainError>;
    async fn list(
        &self,
        query: &ContactListQuery,
    ) -> Result<CursorPage<Contact>, DomainError>;
    async fn upsert(&self, contact: &Contact) -> Result<(), DomainError>;
    /// 删除前须由用例清空相关 Conversation.contact_id
    async fn delete(
        &self,
        account_id: &AccountId,
        id: &ContactId,
    ) -> Result<(), DomainError>;
}
```

### 4.4 MessageRepository

路径：`…/ports/message_repository.rs`

```rust
/// 邮件仓储
#[async_trait]
pub trait MessageRepository: Send + Sync {
    async fn get(
        &self,
        account_id: &AccountId,
        id: &MessageId,
    ) -> Result<Option<MessageWithRecipients>, DomainError>;
    async fn get_by_graph_id(
        &self,
        account_id: &AccountId,
        graph_id: &GraphImmutableId,
    ) -> Result<Option<MessageWithRecipients>, DomainError>;
    /// 写入邮件及收件人行
    async fn upsert_with_recipients(
        &self,
        message: &MessageWithRecipients,
    ) -> Result<(), DomainError>;
    /// 返回实际变更的本地 message id
    async fn mark_read(
        &self,
        account_id: &AccountId,
        message_ids: &[MessageId],
        is_read: bool,
    ) -> Result<Vec<MessageId>, DomainError>;
    /// 标记软删（deleted_remote）
    async fn mark_deleted_remote(
        &self,
        account_id: &AccountId,
        message_ids: &[MessageId],
    ) -> Result<(), DomainError>;
    /// 物理删除（级联清理 recipients / 附着 / jobs）
    async fn delete_physical(
        &self,
        account_id: &AccountId,
        message_ids: &[MessageId],
    ) -> Result<(), DomainError>;
    async fn list_by_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<Vec<Message>, DomainError>;
}
```

### 4.5 GroupRepository

路径：`…/ports/group_repository.rs`

```rust
/// 群组 / 线程归属 / 成员仓储
#[async_trait]
pub trait GroupRepository: Send + Sync {
    async fn get(
        &self,
        account_id: &AccountId,
        id: &GroupId,
    ) -> Result<Option<Group>, DomainError>;
    /// 沿 merged_into_id 解析到根群组
    async fn resolve_root(
        &self,
        account_id: &AccountId,
        id: &GroupId,
    ) -> Result<Option<Group>, DomainError>;
    async fn list(
        &self,
        query: &GroupListQuery,
    ) -> Result<CursorPage<Group>, DomainError>;
    async fn insert(&self, group: &Group) -> Result<(), DomainError>;
    async fn update(&self, group: &Group) -> Result<(), DomainError>;
    async fn list_participants(
        &self,
        account_id: &AccountId,
        group_id: &GroupId,
    ) -> Result<Vec<GroupParticipant>, DomainError>;
    async fn upsert_participants(
        &self,
        participants: &[GroupParticipant],
    ) -> Result<(), DomainError>;
    /// 替换发信成员，并更新 send_members_is_manual
    async fn replace_send_members(
        &self,
        account_id: &AccountId,
        group_id: &GroupId,
        members: &[GroupParticipant],
        send_members_is_manual: bool,
    ) -> Result<(), DomainError>;
    async fn get_thread_link(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<Option<GroupThread>, DomainError>;
    async fn upsert_thread_link(&self, link: &GroupThread) -> Result<(), DomainError>;
    async fn list_threads_by_group(
        &self,
        account_id: &AccountId,
        group_id: &GroupId,
    ) -> Result<Vec<GroupThread>, DomainError>;
}
```

### 4.6 ConversationRepository

路径：`…/ports/conversation_repository.rs`

```rust
/// 会话列表缓存与消息附着仓储
#[async_trait]
pub trait ConversationRepository: Send + Sync {
    async fn get(
        &self,
        account_id: &AccountId,
        id: &ConversationId,
    ) -> Result<Option<Conversation>, DomainError>;
    async fn get_contact_conversation(
        &self,
        account_id: &AccountId,
        address: &EmailAddress,
    ) -> Result<Option<Conversation>, DomainError>;
    async fn get_group_conversation(
        &self,
        account_id: &AccountId,
        group_id: &GroupId,
    ) -> Result<Option<Conversation>, DomainError>;
    async fn list(
        &self,
        query: &ConversationListQuery,
    ) -> Result<CursorPage<ConversationListItem>, DomainError>;
    async fn upsert(&self, conversation: &Conversation) -> Result<(), DomainError>;
    /// 删除联系人前清空引用
    async fn clear_contact_id_refs(
        &self,
        account_id: &AccountId,
        contact_id: &ContactId,
    ) -> Result<(), DomainError>;
    async fn attach_messages(
        &self,
        links: &[ConversationMessageLink],
    ) -> Result<(), DomainError>;
    async fn detach_messages(
        &self,
        account_id: &AccountId,
        conversation_id: &ConversationId,
        message_ids: &[MessageId],
    ) -> Result<(), DomainError>;
    async fn list_message_ids(
        &self,
        account_id: &AccountId,
        conversation_id: &ConversationId,
    ) -> Result<Vec<MessageId>, DomainError>;
    async fn list_messages(
        &self,
        query: &ConversationMessagesQuery,
    ) -> Result<CursorPage<MessageWithRecipients>, DomainError>;
    /// 按当前附着的非软删消息重算 last_* / unread_count
    async fn recompute_projection(
        &self,
        account_id: &AccountId,
        conversation_id: &ConversationId,
    ) -> Result<Conversation, DomainError>;
    async fn find_conversation_ids_by_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<ConversationId>, DomainError>;
}
```

### 4.7 ConversationAggregateJobRepository

路径：`…/ports/conversation_aggregate_job_repository.rs`

```rust
/// 聚合任务仓储（租约领取 / 完成 / 重试）
#[async_trait]
pub trait ConversationAggregateJobRepository: Send + Sync {
    /// 入队或重置为 pending（消息变更需重新聚合时）
    async fn enqueue_or_reset(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        available_at: TimestampMs,
    ) -> Result<(), DomainError>;
    /// 领取一批可处理任务并写入租约
    async fn claim_batch(
        &self,
        now: TimestampMs,
        lease_token: &str,
        lease_expires_at: TimestampMs,
        limit: u32,
    ) -> Result<Vec<ConversationAggregateJob>, DomainError>;
    /// 仅持有当前租约可完成
    async fn complete(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        lease_token: &str,
    ) -> Result<(), DomainError>;
    /// 失败进入 retry，清空租约并设置下次可用时间
    async fn fail_for_retry(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        lease_token: &str,
        error: &str,
        available_at: TimestampMs,
    ) -> Result<(), DomainError>;
    /// 释放过期租约，返回释放条数
    async fn release_expired_leases(&self, now: TimestampMs) -> Result<u32, DomainError>;
}
```

### 4.8 Graph / Auth 出站端口

路径：`…/ports/mail_graph_port.rs` 等

```rust
/// 邮件 Graph 出站端口（仅 infrastructure 实现）
#[async_trait]
pub trait MailGraphPort: Send + Sync {
    /// 文件夹 Delta 一页
    async fn delta_messages(
        &self,
        account_id: &AccountId,
        folder_id: &FolderId,
        delta_or_next_link: Option<&str>,
    ) -> Result<MailDeltaPage, DomainError>;

    /// 按需拉取正文
    async fn get_message_body(
        &self,
        account_id: &AccountId,
        graph_id: &GraphImmutableId,
    ) -> Result<(BodyContentType, String), DomainError>;

    /// 远端标记已读
    async fn patch_message_read(
        &self,
        account_id: &AccountId,
        graph_id: &GraphImmutableId,
        is_read: bool,
    ) -> Result<(), DomainError>;
}

/// Delta 分页结果
pub struct MailDeltaPage {
    pub messages: Vec<GraphMessageChange>,
    pub next_link: Option<String>,
    pub delta_link: Option<String>,
}

/// Graph 邮件变更
pub enum GraphMessageChange {
    /// 插入或更新（已映射为本地实体形状；本地 id 由用例稳定化）
    Upsert(MessageWithRecipients),
    /// 文件夹移出 / 删除信号；须结合当前位置判定
    Remove { graph_id: GraphImmutableId },
}

/// 联系人 Graph 出站端口
#[async_trait]
pub trait ContactGraphPort: Send + Sync {
    async fn delta_contacts(
        &self,
        account_id: &AccountId,
        delta_link: Option<&str>,
    ) -> Result<ContactDeltaPage, DomainError>;
}

pub struct ContactDeltaPage {
    pub upserts: Vec<Contact>,
    pub removed_graph_ids: Vec<String>,
    pub delta_link: Option<String>,
    pub next_link: Option<String>,
}

/// OAuth / 登录出站端口
#[async_trait]
pub trait AuthPort: Send + Sync {
    async fn list_signed_in_accounts(&self) -> Result<Vec<Account>, DomainError>;
    async fn sign_in(&self) -> Result<Account, DomainError>;
    async fn sign_out(&self, account_id: &AccountId) -> Result<(), DomainError>;
    async fn get_access_token(&self, account_id: &AccountId) -> Result<String, DomainError>;
}
```

---

## 5. Domain 服务接口

路径：`src-tauri/domain/services/`

```rust
/// 从账户解析 self 排除集合
pub trait SelfAddressResolver: Send + Sync {
    fn resolve(&self, account: &Account) -> SelfAddressSet;
}

/// 出站判定、联系人投影地址、群组观测成员等纯规则
pub trait MessageAttributionService: Send + Sync {
    /// 是否本账户发出
    fn is_outbound(&self, message: &Message, self_set: &SelfAddressSet) -> bool;
    /// 参与联系人会话投影的对端地址
    fn peer_addresses_for_contact_projection(
        &self,
        message: &MessageWithRecipients,
        self_set: &SelfAddressSet,
    ) -> Vec<EmailAddress>;
    /// 观测到的群组成员（地址 + 显示名）
    fn observed_participants(
        &self,
        message: &MessageWithRecipients,
        self_set: &SelfAddressSet,
    ) -> Vec<(EmailAddress, String)>;
}

/// 会话 / 群组归组（写入路径由 Application 开事务调用）
#[async_trait]
pub trait ConversationAggregationService: Send + Sync {
    /// 要求 message.thread_id 已非空；否则用例重新入队
    async fn aggregate_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), DomainError>;

    /// 重算群组观测成员（保留手动发信成员）
    async fn recompute_group_observed_participants(
        &self,
        account_id: &AccountId,
        group_id: &GroupId,
    ) -> Result<(), DomainError>;
}
```

---

## 6. Application 用例接口

路径：`src-tauri/application/use_cases/`  
编排仓储与领域服务；对前端 / `commands/` 暴露。不含 Graph SDK。

### 6.1 AccountUseCase

```rust
/// 账户用例
#[async_trait]
pub trait AccountUseCase: Send + Sync {
    async fn list_accounts(&self) -> Result<Vec<Account>, AppError>;
    async fn get_account(&self, account_id: &AccountId) -> Result<Account, AppError>;
    async fn sign_in(&self) -> Result<Account, AppError>;
    async fn sign_out(&self, account_id: &AccountId) -> Result<(), AppError>;
    async fn update_display_settings(
        &self,
        cmd: UpdateAccountDisplaySettingsCommand,
    ) -> Result<Account, AppError>;
}
```

### 6.2 SyncUseCase

```rust
/// 同步用例
#[async_trait]
pub trait SyncUseCase: Send + Sync {
    async fn trigger_mail_sync(&self, cmd: TriggerMailSyncCommand) -> Result<(), AppError>;
    async fn trigger_contact_sync(&self, cmd: TriggerContactSyncCommand) -> Result<(), AppError>;
    async fn get_mail_sync_progress(
        &self,
        account_id: &AccountId,
    ) -> Result<MailSyncProgress, AppError>;
    async fn get_contact_sync_progress(
        &self,
        account_id: &AccountId,
    ) -> Result<ContactSyncProgress, AppError>;
}
```

### 6.3 ContactUseCase

```rust
/// 联系人用例
#[async_trait]
pub trait ContactUseCase: Send + Sync {
    async fn list_contacts(
        &self,
        query: ContactListQuery,
    ) -> Result<CursorPage<Contact>, AppError>;
    async fn get_contact(
        &self,
        account_id: &AccountId,
        contact_id: &ContactId,
    ) -> Result<Contact, AppError>;
    async fn get_contact_by_address(
        &self,
        account_id: &AccountId,
        address: &EmailAddress,
    ) -> Result<Option<Contact>, AppError>;
}
```

### 6.4 ConversationUseCase

```rust
/// 会话用例（主导航）
#[async_trait]
pub trait ConversationUseCase: Send + Sync {
    async fn list_conversations(
        &self,
        query: ConversationListQuery,
    ) -> Result<CursorPage<ConversationListItem>, AppError>;
    async fn get_conversation(
        &self,
        account_id: &AccountId,
        conversation_id: &ConversationId,
    ) -> Result<ConversationListItem, AppError>;
    async fn list_messages(
        &self,
        query: ConversationMessagesQuery,
    ) -> Result<CursorPage<MessageWithRecipients>, AppError>;
    async fn ensure_contact_conversation(
        &self,
        cmd: EnsureContactConversationCommand,
    ) -> Result<Conversation, AppError>;
}
```

### 6.5 MessageUseCase

```rust
/// 邮件用例
#[async_trait]
pub trait MessageUseCase: Send + Sync {
    async fn get_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<MessageWithRecipients, AppError>;
    /// 按需拉取正文并回写本地
    async fn ensure_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<MessageWithRecipients, AppError>;
    async fn mark_read(&self, cmd: MarkMessagesReadCommand) -> Result<(), AppError>;
}
```

### 6.6 GroupUseCase

```rust
/// 群组用例
#[async_trait]
pub trait GroupUseCase: Send + Sync {
    async fn list_groups(
        &self,
        query: GroupListQuery,
    ) -> Result<CursorPage<Group>, AppError>;
    async fn get_group(
        &self,
        account_id: &AccountId,
        group_id: &GroupId,
    ) -> Result<Group, AppError>;
    async fn list_participants(
        &self,
        account_id: &AccountId,
        group_id: &GroupId,
    ) -> Result<Vec<GroupParticipant>, AppError>;
    async fn create_manual_group(
        &self,
        cmd: CreateManualGroupCommand,
    ) -> Result<Group, AppError>;
    async fn rename_group(&self, cmd: RenameGroupCommand) -> Result<Group, AppError>;
    async fn merge_groups(&self, cmd: MergeGroupsCommand) -> Result<Group, AppError>;
    async fn move_thread_to_group(
        &self,
        cmd: MoveThreadToGroupCommand,
    ) -> Result<(), AppError>;
    async fn update_send_members(
        &self,
        cmd: UpdateGroupSendMembersCommand,
    ) -> Result<Vec<GroupParticipant>, AppError>;
}
```

### 6.7 AggregateWorkerUseCase（内部）

```rust
/// 聚合后台用例：由同步完成或定时触发；不直接暴露给 UI
#[async_trait]
pub trait AggregateWorkerUseCase: Send + Sync {
    /// 处理一批可用任务，返回成功完成数
    async fn process_available_jobs(&self, limit: u32) -> Result<u32, AppError>;
}
```

---

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

路径：`src-tauri/domain/error.rs`、`src-tauri/application/error.rs`、`src/domain/errors.ts`

```rust
/// 领域层错误
pub enum DomainError {
    /// 实体不存在
    NotFound { entity: &'static str, id: String },
    /// 冲突（合并环、重复根等）
    Conflict { message: String },
    /// 不变量破坏
    InvariantViolation { message: String },
    /// 本地存储
    Storage { message: String },
    /// 外部系统（经端口返回后的映射）
    External { message: String },
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

| 数据表 | 实体文件 | 端口文件 |
|---|---|---|
| `accounts` | `domain/entities/account.rs` | `ports/account_repository.rs`、`auth_port.rs` |
| `mail_folder_sync_state` | `domain/entities/sync_state.rs` | `ports/mail_folder_sync_state_repository.rs`、`mail_graph_port.rs` |
| `contacts` | `domain/entities/contact.rs` | `ports/contact_repository.rs`、`contact_graph_port.rs` |
| `messages` / `message_recipients` | `domain/entities/message.rs` | `ports/message_repository.rs` |
| `groups` / `group_*` | `domain/entities/group.rs` | `ports/group_repository.rs` |
| `conversations` / `conversation_messages` | `domain/entities/conversation.rs` | `ports/conversation_repository.rs` |
| `conversation_aggregate_jobs` | `domain/entities/aggregate_job.rs` | `ports/conversation_aggregate_job_repository.rs` |

| 前端能力 | Port 文件 |
|---|---|
| 账户 | `src/ports/accountPort.ts` |
| 同步 | `src/ports/syncPort.ts` |
| 联系人 | `src/ports/contactPort.ts` |
| 会话 | `src/ports/conversationPort.ts` |
| 邮件 | `src/ports/messagePort.ts` |
| 群组 | `src/ports/groupPort.ts` |

归属真相：`group_thread` + `conversation_messages`。列表读 `ConversationRepository::list`；详情消息走 `list_messages`。

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

## 11. 落地顺序建议

1. 按 §0 建空目录与 `mod.rs` / 导出
2. 落 Domain 值对象、实体、`DomainError`（§1–§2、§8）
3. 落 Domain 仓储 / Graph / Auth trait（§4）与领域服务 trait（§5）
4. 落 Application DTO 与用例 trait（§3、§6）
5. 落前端 `domain/` 类型与 `ports/`（§7）
6. 再写 infrastructure / Adapter / commands（均不在本文）

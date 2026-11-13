# V1.0.0 数据模型

存储：**SQLite**。所有写连接启用 `PRAGMA foreign_keys=ON`。

约定：

- ID 均为 **TEXT**（与 Graph id / 本地生成 id 对齐）。
- 本地实体 ID 与远端标识分离：`messages.id` 为稳定本地 ID，`messages.graph_id` 为 Graph ImmutableId；所有 ID 区分大小写。
- 所有群组 / 会话相关表均含非空 `account_id`（第一版租户隔离）。
- 时间戳均为 **INTEGER**（Unix 毫秒，除非另行说明）。
- 布尔用 `INTEGER` + `CHECK(... IN (0, 1))`。
- 邮箱地址规范化：`trim + lowercase`；比较列使用 `COLLATE NOCASE`。

依赖顺序（建表）：`accounts` → `mail_folder_sync_state` / `contacts` → `messages` → `message_recipients` / `groups` → `group_*` / `conversations` → `conversation_*` / jobs。

---

## 总览

| 表 | 职责 |
|---|---|
| `accounts` | Outlook 账户（租户根；含 self 别名 JSON） |
| `mail_folder_sync_state` | 按账户、文件夹保存邮件增量游标及同步状态 |
| `contacts` | 通讯录缓存 |
| `messages` | 邮件真相（Graph 同步落库） |
| `message_recipients` | 邮件收件人拆行 |
| `groups` | 业务群组（自动/手动） |
| `group_thread` | 线程 → 群组主归属 |
| `group_participants` | 群组成员投影 |
| `conversations` | 会话列表缓存（contact / group） |
| `conversation_messages` | 消息 ↔ 会话多对多附着 |
| `conversation_aggregate_jobs` | 聚合任务（失败恢复） |

归属真相：`group_thread` + `conversation_messages`。`conversations` 只缓存列表派生字段。

---

## `accounts`

```sql
CREATE TABLE accounts (
    id              TEXT PRIMARY KEY,
    email           TEXT NOT NULL COLLATE NOCASE,
    display_name    TEXT NOT NULL DEFAULT '',
    -- self 额外身份（规范化地址 JSON 数组），不单建 account_aliases 表
    -- 例：'["alias@contoso.com","old@contoso.com"]'；空为 '[]'
    aliases_json    TEXT NOT NULL DEFAULT '[]',
    -- Graph / OAuth 侧标识（按实现可拆 token 表；V1 最小落库）
    tenant_id       TEXT,
    home_account_id TEXT,
    -- 联系人同步游标；邮件游标见 mail_folder_sync_state
    contact_delta_link TEXT,
    mail_synced_at INTEGER,          -- 最近一次账户邮件同步轮次全部成功结束时间
    contact_synced_at INTEGER,       -- 最近一次联系人同步成功结束时间
    mail_sync_status TEXT NOT NULL DEFAULT 'idle'
        CHECK(mail_sync_status IN ('idle', 'running', 'error')),
    mail_sync_error TEXT,            -- 最近一次失败原因；成功时清空
    -- 显示配置
    show_group_messages_in_contact_conversation
        INTEGER NOT NULL DEFAULT 0
        CHECK(show_group_messages_in_contact_conversation IN (0, 1)),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    UNIQUE (email)
);

CREATE INDEX idx_accounts_email ON accounts(email);
```

- self 排除集合 = `{email} ∪ aliases_json`（元素均 trim + lowercase）。不做 Send-As；配置 UI 可后置。
- `mail_synced_at` 是账户级成功时间，不是 Graph 查询游标；邮件增量游标按文件夹保存在 `mail_folder_sync_state`。
- 一个账户同步轮次覆盖该轮确定的全部跟踪文件夹；仅当这些文件夹均完成本轮同步时，才更新 `mail_synced_at`、清空 `mail_sync_error`、置 `mail_sync_status='idle'`。任一文件夹失败则账户状态为 `error`，不推进账户成功时间，已成功文件夹的进度保留。
- `show_group_messages_in_contact_conversation`：群组消息是否同时出现在联系人会话投影中（默认否）。
- Token 机密不进本表明文长期存放时，可另表加密存储；本模型不展开密钥方案。

---

## `mail_folder_sync_state`

Graph 邮件 Delta 的作用域是单个文件夹（[Graph 官方说明](https://learn.microsoft.com/en-us/graph/delta-query-messages)）。此表仅描述同步范围与进度，不承担文件夹导航或会话归属。

```sql
CREATE TABLE mail_folder_sync_state (
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id       TEXT NOT NULL,
    delta_link      TEXT,
    next_link       TEXT,
    synced_at       INTEGER,
    sync_status     TEXT NOT NULL DEFAULT 'idle'
        CHECK(sync_status IN ('idle', 'running', 'error')),
    last_error      TEXT,
    updated_at      INTEGER NOT NULL,
    PRIMARY KEY (account_id, folder_id)
);
```

- 每行代表一个被跟踪文件夹；不同文件夹的游标不可互用。`messages.folder_id` 是邮件当前位置，不强制引用本表，允许邮件先于文件夹同步状态落库。
- `delta_link` 表示最近完成轮次的增量游标；`next_link` 表示当前未完成轮次的分页续传位置。初次同步前二者均可为空。
- 一页邮件变更及其聚合任务与该页续传位置必须原子提交；最终页提交时替换 `delta_link`、清空 `next_link`，并更新该文件夹的 `synced_at`。失败不得使游标越过尚未提交的数据。
- 游标失效时清空该文件夹游标，重新建立同步基线；在基线完成前保留原成功时间。文件夹移出跟踪范围前，须完成其邮件的保留、迁移或删除判定，避免留下失去同步来源的数据。
- 文件夹 Delta 中的移出记录不等于邮件永久删除；应结合邮件当前位置判定。邮箱内移动只更新邮件的位置，不改变本地消息身份及会话附着。

---

## `contacts`

通讯录本地缓存；联系人会话通过 `normalized_address` / `contact_id` 关联。

```sql
CREATE TABLE contacts (
    id                  TEXT PRIMARY KEY,
    account_id          TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    graph_id            TEXT,
    display_name        TEXT NOT NULL DEFAULT '',
    normalized_address  TEXT NOT NULL COLLATE NOCASE,
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL,
    UNIQUE (account_id, id),
    UNIQUE (account_id, graph_id)
);

CREATE UNIQUE INDEX idx_contacts_account_address
    ON contacts(account_id, normalized_address);

CREATE INDEX idx_contacts_account ON contacts(account_id);
```

- 一人多邮箱若后续需要，再拆 `contact_emails`；V1 以主地址一行表示。
- `graph_id` 可空：允许本地仅按地址建的占位联系人。

---

## `messages`（邮件真相）

Graph Message 同步落库。会话/群组**不**改写本表业务归属；归属由 `group_thread` / `conversation_messages` 维护。

```sql
CREATE TABLE messages (
    id                      TEXT NOT NULL,
    account_id              TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,

    -- Graph ImmutableId；本地尚未同步的草稿可为空
    graph_id                TEXT,

    -- Graph 线程 / 会话 id（Outlook conversationId）；群组归组主键依据
    thread_id               TEXT CHECK(thread_id IS NULL OR length(trim(thread_id)) > 0),

    -- Graph 元数据
    graph_change_key        TEXT,
    internet_message_id     TEXT,
    conversation_index      BLOB,

    subject                 TEXT NOT NULL DEFAULT '',
    body_preview            TEXT NOT NULL DEFAULT '',
    body_content_type       TEXT NOT NULL DEFAULT 'text'
        CHECK(body_content_type IN ('text', 'html')),
    body_content            TEXT NOT NULL DEFAULT '',

    from_address            TEXT COLLATE NOCASE,
    from_name               TEXT NOT NULL DEFAULT '',

    received_at             INTEGER,
    sent_at                 INTEGER,
    created_at_graph        INTEGER,

    is_read                 INTEGER NOT NULL DEFAULT 0 CHECK(is_read IN (0, 1)),
    is_draft                INTEGER NOT NULL DEFAULT 0 CHECK(is_draft IN (0, 1)),
    is_outbound             INTEGER NOT NULL DEFAULT 0 CHECK(is_outbound IN (0, 1)),
    has_attachments         INTEGER NOT NULL DEFAULT 0 CHECK(has_attachments IN (0, 1)),
    importance              TEXT NOT NULL DEFAULT 'normal'
        CHECK(importance IN ('low', 'normal', 'high')),

    -- 文件夹仅作同步/过滤投影，非信息架构主轴
    folder_id               TEXT,
    parent_folder_name      TEXT,

    -- 本地同步状态
    sync_status             TEXT NOT NULL DEFAULT 'synced'
        CHECK(sync_status IN ('synced', 'pending_local', 'deleted_remote')),
    local_updated_at        INTEGER NOT NULL,
    remote_updated_at       INTEGER,

    PRIMARY KEY (account_id, id),
    UNIQUE (account_id, graph_id)
);

CREATE INDEX idx_messages_account_thread
    ON messages(account_id, thread_id);

CREATE INDEX idx_messages_account_received
    ON messages(account_id, received_at DESC);

CREATE INDEX idx_messages_internet_message_id
    ON messages(account_id, internet_message_id);

CREATE INDEX idx_messages_folder
    ON messages(account_id, folder_id);
```

字段说明：

| 字段 | 说明 |
|---|---|
| `(account_id, id)` | 主键；`id` 为生成后不变的本地消息 ID，所有本地关联均引用它 |
| `graph_id` | Graph ImmutableId；账户内唯一；仅未取得远端身份的本地消息可空 |
| `thread_id` | Graph `conversationId`；`group_thread.thread_id` 与之对齐；暂缺时为 NULL，等待补齐后聚合 |
| `internet_message_id` | RFC Message-ID；去重/外部分诊备用 |
| `body_*` | V1 可先同步 `bodyPreview`，正文按需拉取后回写 |
| `is_outbound` | 是否本账户发出（结合 `email ∪ aliases_json` 判定后落库） |
| `folder_id` | Graph 邮件文件夹；**不是**主导航模型 |
| `sync_status` | 本地草稿/软删与远端对齐用 |

不变量：

- 同一账户下 `id` 唯一（已由主键保证）。
- 远端邮件通过 `(account_id, graph_id)` 识别同一记录；同一邮箱内移动、草稿发送及远端更新均不改变本地 `id`。本地草稿取得远端身份后只补充 `graph_id`，不替换主键。
- 所有返回或使用邮件远端标识的 Graph 请求统一使用 `Prefer: IdType="ImmutableId"`；默认 Graph ID 与 ImmutableId 不混用。该稳定性限于同一邮箱，跨邮箱归档或重新导入不保证保留远端身份（[Graph 官方说明](https://learn.microsoft.com/en-us/graph/outlook-immutable-id)）。
- 聚合入会话前，`thread_id` 必须非空；若 Graph 暂缺，入队 job 待补齐后再归组。
- 物理删除消息时，`ON DELETE CASCADE` 清理 `message_recipients`、`conversation_messages` 与 aggregate jobs；删除前须确定受影响会话与群组，并在同一事务重算列表缓存及成员观测投影。
- `sync_status='deleted_remote'` 的软删消息不参与会话展示、未读计数、最新消息及成员观测；标记软删与相关投影更新保持原子性。成员重算只更新观测信息，保留手动配置的发信成员。

### `message_recipients`

收件人拆行，便于按地址反查与群组成员观测。

```sql
CREATE TABLE message_recipients (
    account_id          TEXT NOT NULL,
    message_id          TEXT NOT NULL,
    role                TEXT NOT NULL CHECK(role IN ('to', 'cc', 'bcc', 'reply_to')),
    seq                 INTEGER NOT NULL DEFAULT 0,
    normalized_address  TEXT NOT NULL COLLATE NOCASE,
    display_name        TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (account_id, message_id, role, normalized_address),
    FOREIGN KEY (account_id, message_id)
        REFERENCES messages(account_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_message_recipients_address
    ON message_recipients(account_id, normalized_address);
```

---

## `groups`

```sql
CREATE TABLE groups (
    id              TEXT PRIMARY KEY,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name            TEXT NOT NULL DEFAULT '',
    name_is_manual  INTEGER NOT NULL DEFAULT 0 CHECK(name_is_manual IN (0, 1)),
    source          TEXT NOT NULL CHECK(source IN ('auto', 'manual')),
    send_members_is_manual INTEGER NOT NULL DEFAULT 0 CHECK(send_members_is_manual IN (0, 1)),
    merged_into_id  TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    UNIQUE (account_id, id),
    FOREIGN KEY (account_id, merged_into_id) REFERENCES groups(account_id, id),
    CHECK (merged_into_id IS NULL OR merged_into_id != id)
);

CREATE INDEX idx_groups_account ON groups(account_id);
CREATE INDEX idx_groups_merged_into ON groups(merged_into_id);
```

- `source`：仅表示群组**如何诞生**（自动 vs 用户空手创建），**不是**线程归组的第二套规则。
- `merged_into_id`：被吸收群组指向存活群组；列表与解析只认根群组。

---

## `group_thread`（账户内单一主归属）

```sql
CREATE TABLE group_thread (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    group_id   TEXT NOT NULL,
    thread_id  TEXT NOT NULL,
    linked_at  INTEGER NOT NULL,
    PRIMARY KEY (account_id, thread_id),
    FOREIGN KEY (account_id, group_id) REFERENCES groups(account_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_group_thread_group ON group_thread(group_id);
```

不变量：

- 主键 `(account_id, thread_id)` ⇒ 该账户下每个线程最多一个主群组。
- **不再有** `link_source` 列；避免 AUTO/MANUAL 双行。
- 用户「把线程挪到另一群组」= 更新本行的 `group_id`，并在同一事务迁移 `conversation_messages`、重算两侧投影。

---

## `group_participants`

```sql
CREATE TABLE group_participants (
    account_id          TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    group_id            TEXT NOT NULL,
    normalized_address  TEXT NOT NULL COLLATE NOCASE,
    display_name        TEXT NOT NULL DEFAULT '',
    is_observed         INTEGER NOT NULL DEFAULT 1 CHECK(is_observed IN (0, 1)),
    is_send_member      INTEGER NOT NULL DEFAULT 0 CHECK(is_send_member IN (0, 1)),
    first_seen_at       INTEGER NOT NULL,
    last_seen_at        INTEGER NOT NULL,
    PRIMARY KEY (group_id, normalized_address),
    FOREIGN KEY (account_id, group_id) REFERENCES groups(account_id, id) ON DELETE CASCADE
);
```

---

## `conversations`

```sql
CREATE TABLE conversations (
    id                 TEXT PRIMARY KEY,
    account_id         TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind               TEXT NOT NULL CHECK(kind IN ('contact', 'group')),
    normalized_address TEXT COLLATE NOCASE,
    contact_id         TEXT,
    group_id           TEXT,
    last_message_at    INTEGER,
    last_message_id    TEXT,
    unread_count       INTEGER NOT NULL DEFAULT 0 CHECK(unread_count >= 0),
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL,
    UNIQUE (account_id, id),
    FOREIGN KEY (account_id, contact_id) REFERENCES contacts(account_id, id),
    FOREIGN KEY (account_id, group_id) REFERENCES groups(account_id, id) ON DELETE CASCADE,
    CHECK (
        (kind = 'contact' AND normalized_address IS NOT NULL AND group_id IS NULL)
        OR
        (kind = 'group' AND group_id IS NOT NULL AND normalized_address IS NULL AND contact_id IS NULL)
    )
);

CREATE UNIQUE INDEX idx_conversations_contact
    ON conversations(account_id, normalized_address)
    WHERE kind = 'contact';

CREATE UNIQUE INDEX idx_conversations_group
    ON conversations(account_id, group_id)
    WHERE kind = 'group';
```

- `account_id` **NOT NULL**：与「第一版租户隔离」一致；唯一索引不含 NULL 歧义。
- Conversation **只缓存**列表所需派生字段；归属真相在 `group_thread` + `conversation_messages`。
- `contact_id` 可空；非空时必须指向同账户联系人。删除联系人时，在同一事务先清空相关会话的 `contact_id`，再删除联系人；保留会话地址与邮件历史。复合外键不使用 `ON DELETE SET NULL`，以免同时清空 `account_id`。
- `last_message_id` 必须指向当前会话附着的、非软删消息；`last_message_at` 与之对应，`unread_count` 仅统计当前会话内非软删且未读的消息。消息删除、软删、已读状态变化或附着迁移后均须保持这些派生值一致。
- V1 保留空会话：无可见消息时 `last_message_id`、`last_message_at` 均为 NULL，`unread_count=0`；不会因最后一封消息删除而丢失联系人关联、群组或手动配置。

可选增强（实现期）：`last_message_id` 与 `messages` 复合外键——需 `(account_id, last_message_id)`，V1 可先应用层保证一致性。

---

## `conversation_messages`

```sql
CREATE TABLE conversation_messages (
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL,
    message_id      TEXT NOT NULL,
    attached_at     INTEGER NOT NULL,
    PRIMARY KEY (conversation_id, message_id),
    FOREIGN KEY (account_id, conversation_id) REFERENCES conversations(account_id, id) ON DELETE CASCADE,
    FOREIGN KEY (account_id, message_id) REFERENCES messages(account_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_conversation_messages_message
    ON conversation_messages(message_id);
```

`messages` 的复合主键 `(account_id, id)` 已满足本表及 jobs 的复合外键引用要求，无需另建重复唯一索引。

复合外键保证关联两端账户相同；服务入口仍须校验调用账户、目标为存活根群组及合并无环，不能把非空账户字段当成隔离保证。

---

## 账户显示配置

已并入 `accounts.show_group_messages_in_contact_conversation`（见上）。若后续设置膨胀，再拆 `account_settings` JSON/表。

---

## `conversation_aggregate_jobs`（聚合处理状态 / 失败恢复）

轻量任务表即可，无需消息中间件：

```sql
CREATE TABLE conversation_aggregate_jobs (
    message_id   TEXT NOT NULL,
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    status       TEXT NOT NULL CHECK(status IN ('pending', 'processing', 'completed', 'retry')),
    attempts     INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
    last_error   TEXT,
    available_at INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    lease_token  TEXT,
    lease_expires_at INTEGER,
    PRIMARY KEY (account_id, message_id),
    CHECK (
        (status = 'processing' AND lease_token IS NOT NULL AND lease_expires_at IS NOT NULL)
        OR
        (status != 'processing' AND lease_token IS NULL AND lease_expires_at IS NULL)
    ),
    FOREIGN KEY (account_id, message_id) REFERENCES messages(account_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_conversation_aggregate_jobs_poll
    ON conversation_aggregate_jobs(status, available_at);

CREATE INDEX idx_conversation_aggregate_jobs_lease
    ON conversation_aggregate_jobs(lease_expires_at)
    WHERE status = 'processing';
```

状态与恢复不变量：

- 每个 `(account_id, message_id)` 最多一条任务；所有任务标识、领取及完成判定均包含账户维度，`message_id` 引用本地消息 ID。
- `pending` 为首次待处理，`retry` 为失败后待重试；二者由 `available_at` 表示最早可处理时间。进入 `processing` 时记录本次领取的唯一 `lease_token` 与租约到期时间，并增加 `attempts`。
- `processing` 租约到期后可回到 `retry`，清空租约字段；崩溃遗留任务因此可恢复。只有持有当前有效租约的处理结果可提交，旧租约不得覆盖重新领取后的结果。
- 聚合结果与任务置为 `completed` 必须原子提交；完成或失败退出 `processing` 时清空租约字段，失败保留 `last_error` 并设置下次 `available_at`。
- 同一消息发生需要重新聚合的变更时，复用任务并回到 `pending`、清除旧租约；消息变更与任务重置必须原子提交，防止旧处理结果覆盖新状态。重复处理不得产生重复附着或累计计数。

---

## ER 关系（逻辑）

```
accounts 1──* contacts
accounts 1──* mail_folder_sync_state
accounts 1──* messages 1──* message_recipients
accounts 1──* groups 1──* group_participants
groups   1──* group_thread          (thread_id → messages.thread_id 逻辑关联)
accounts 1──* conversations
conversations 1──* conversation_messages *──1 messages
messages 1──0..1 conversation_aggregate_jobs
```

说明：`group_thread.thread_id` 与 `messages.thread_id` 为**逻辑**关联（按账户+线程归组），不建强制 FK 到 messages，因线程可在首封消息前由手动规则预建。

---

## V1 刻意不做

- 附件二进制大表（仅 `has_attachments`；附件元数据可下版加 `message_attachments`）
- Send-As / 多发信身份
- 文件夹树作为主导航持久模型
- 跨账户统一会话
- `link_source` / 线程双归属

---

## 实现检查

- [ ] 所有写连接 `PRAGMA foreign_keys=ON`
- [ ] 邮件 Delta 游标按 `(account_id, folder_id)` 保存，游标与对应数据变更原子提交
- [ ] 邮件本地 ID 稳定，远端身份统一为 ImmutableId
- [ ] `messages` 先于 `conversation_messages` / jobs 建表
- [ ] 归组与挪线程在同一事务内更新 `group_thread` + `conversation_messages` + 两侧 `conversations` 投影
- [ ] self 排除使用 `accounts.email ∪ accounts.aliases_json`
- [ ] 邮件同步成功后更新 `accounts.mail_synced_at`（失败不推进）
- [ ] 联系人引用使用账户复合外键；删除联系人保留会话历史
- [ ] 删除 / 软删 / 已读变更后更新受影响会话与群组投影
- [ ] 聚合任务按账户、消息唯一；租约过期可恢复，旧租约结果不可提交
- [ ] 列表读 `conversations`，详情消息走 `conversation_messages` → `messages`

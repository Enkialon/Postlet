-- V1 schema：Session 一等公民；Contact/Group 为会话投影关联；消息只属一个 Session。

CREATE TABLE accounts (
    id              TEXT PRIMARY KEY,
    email           TEXT NOT NULL COLLATE NOCASE,
    display_name    TEXT NOT NULL DEFAULT '',
    aliases_json    TEXT NOT NULL DEFAULT '[]',
    tenant_id       TEXT,
    home_account_id TEXT,
    contact_delta_link TEXT,
    mail_sync_triggered_at INTEGER,
    contact_synced_at INTEGER,
    mail_sync_status TEXT NOT NULL DEFAULT 'idle'
        CHECK(mail_sync_status IN ('idle', 'running', 'error')),
    mail_sync_error TEXT,
    is_signed_in    INTEGER NOT NULL DEFAULT 1 CHECK(is_signed_in IN (0, 1)),
    include_in_all  INTEGER NOT NULL DEFAULT 1 CHECK(include_in_all IN (0, 1)),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    UNIQUE (email)
);

CREATE INDEX idx_accounts_email ON accounts(email);

CREATE TABLE account_mail_transport (
    account_id          TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    receive_protocol    TEXT NOT NULL DEFAULT 'graph'
        CHECK(receive_protocol IN ('graph', 'imap')),
    send_protocol       TEXT NOT NULL DEFAULT 'graph'
        CHECK(send_protocol IN ('graph', 'smtp')),
    auth_kind           TEXT NOT NULL DEFAULT 'oauth2'
        CHECK(auth_kind IN ('password', 'oauth2')),
    credential_ref      TEXT,
    imap_host           TEXT,
    imap_port           INTEGER,
    imap_tls            TEXT
        CHECK(imap_tls IS NULL OR imap_tls IN ('implicit', 'starttls', 'plain')),
    smtp_host           TEXT,
    smtp_port           INTEGER,
    smtp_tls            TEXT
        CHECK(smtp_tls IS NULL OR smtp_tls IN ('implicit', 'starttls', 'plain')),
    username            TEXT,
    imap_idle_supported INTEGER
        CHECK(imap_idle_supported IS NULL OR imap_idle_supported IN (0, 1)),
    updated_at          INTEGER NOT NULL
);

CREATE TABLE mail_folder_sync_state (
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id       TEXT NOT NULL,
    generation      TEXT,
    delta_link      TEXT,
    next_link       TEXT,
    synced_at       INTEGER,
    sync_status     TEXT NOT NULL DEFAULT 'idle'
        CHECK(sync_status IN ('idle', 'running', 'error')),
    last_error      TEXT,
    updated_at      INTEGER NOT NULL,
    parent_folder_id TEXT,
    display_name    TEXT NOT NULL DEFAULT '',
    sync_enabled    INTEGER NOT NULL DEFAULT 0 CHECK(sync_enabled IN (0, 1)),
    PRIMARY KEY (account_id, folder_id)
);

-- 人表：graph/manual = 真联系人（通讯录）；observed = 邮件/群观测短名（非通讯录）
CREATE TABLE contacts (
    id                  TEXT NOT NULL,
    account_id          TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    graph_id            TEXT,
    display_name        TEXT NOT NULL DEFAULT '',
    normalized_address  TEXT NOT NULL COLLATE NOCASE,
    source              TEXT NOT NULL CHECK(source IN ('graph', 'manual', 'observed')),
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL,
    PRIMARY KEY (account_id, id),
    UNIQUE (account_id, graph_id)
);

CREATE UNIQUE INDEX idx_contacts_account_address
    ON contacts(account_id, normalized_address);

CREATE INDEX idx_contacts_account ON contacts(account_id);

-- 群档案（群聊 Session 的关联投影；合并在 sessions.merged_into_id）
CREATE TABLE groups (
    id              TEXT NOT NULL,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name            TEXT NOT NULL DEFAULT '',
    name_is_manual  INTEGER NOT NULL DEFAULT 0 CHECK(name_is_manual IN (0, 1)),
    source          TEXT NOT NULL CHECK(source IN ('auto', 'manual')),
    send_members_is_manual INTEGER NOT NULL DEFAULT 0 CHECK(send_members_is_manual IN (0, 1)),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    PRIMARY KEY (account_id, id)
);

CREATE INDEX idx_groups_account ON groups(account_id);

CREATE TABLE group_participants (
    account_id          TEXT NOT NULL,
    group_id            TEXT NOT NULL,
    normalized_address  TEXT NOT NULL COLLATE NOCASE,
    display_name        TEXT NOT NULL DEFAULT '',
    is_observed         INTEGER NOT NULL DEFAULT 1 CHECK(is_observed IN (0, 1)),
    is_send_member      INTEGER NOT NULL DEFAULT 0 CHECK(is_send_member IN (0, 1)),
    first_seen_at       INTEGER NOT NULL,
    last_seen_at        INTEGER NOT NULL,
    PRIMARY KEY (account_id, group_id, normalized_address),
    FOREIGN KEY (account_id, group_id) REFERENCES groups(account_id, id) ON DELETE CASCADE
);

-- 会话一等公民：一张表，private / group
CREATE TABLE sessions (
    id                 TEXT NOT NULL,
    account_id         TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind               TEXT NOT NULL CHECK(kind IN ('private', 'group')),
    normalized_address TEXT COLLATE NOCASE,
    contact_id         TEXT,
    group_id           TEXT,
    merged_into_id     TEXT,
    last_message_at    INTEGER,
    last_message_id    TEXT,
    unread_count       INTEGER NOT NULL DEFAULT 0 CHECK(unread_count >= 0),
    hidden_at          INTEGER,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL,
    PRIMARY KEY (account_id, id),
    FOREIGN KEY (account_id, contact_id) REFERENCES contacts(account_id, id),
    FOREIGN KEY (account_id, group_id) REFERENCES groups(account_id, id) ON DELETE CASCADE,
    FOREIGN KEY (account_id, merged_into_id) REFERENCES sessions(account_id, id),
    CHECK (merged_into_id IS NULL OR merged_into_id != id),
    CHECK (
        (kind = 'private' AND normalized_address IS NOT NULL AND group_id IS NULL)
        OR
        (kind = 'group' AND group_id IS NOT NULL AND normalized_address IS NULL AND contact_id IS NULL)
    )
);

CREATE UNIQUE INDEX idx_sessions_private
    ON sessions(account_id, normalized_address)
    WHERE kind = 'private';

CREATE UNIQUE INDEX idx_sessions_group
    ON sessions(account_id, group_id)
    WHERE kind = 'group';

CREATE INDEX idx_sessions_merged_into ON sessions(account_id, merged_into_id);
CREATE INDEX idx_sessions_list
    ON sessions(account_id, last_message_at DESC)
    WHERE merged_into_id IS NULL AND hidden_at IS NULL;

CREATE TABLE messages (
    id                      TEXT NOT NULL,
    account_id              TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    session_id              TEXT NOT NULL,
    remote_key              TEXT,
    internet_message_id     TEXT,
    in_reply_to             TEXT,
    subject                 TEXT NOT NULL DEFAULT '',
    body_preview            TEXT NOT NULL DEFAULT '',
    body_content_type       TEXT NOT NULL DEFAULT 'text'
        CHECK(body_content_type IN ('text', 'html')),
    body_content            TEXT,
    from_address            TEXT COLLATE NOCASE,
    from_name               TEXT NOT NULL DEFAULT '',
    received_at             INTEGER,
    sent_at                 INTEGER,
    is_read                 INTEGER NOT NULL DEFAULT 0 CHECK(is_read IN (0, 1)),
    is_draft                INTEGER NOT NULL DEFAULT 0 CHECK(is_draft IN (0, 1)),
    remote_is_read          INTEGER CHECK(remote_is_read IS NULL OR remote_is_read IN (0, 1)),
    remote_is_draft         INTEGER CHECK(remote_is_draft IS NULL OR remote_is_draft IN (0, 1)),
    remote_is_deleted       INTEGER CHECK(remote_is_deleted IS NULL OR remote_is_deleted IN (0, 1)),
    is_outbound             INTEGER NOT NULL DEFAULT 0 CHECK(is_outbound IN (0, 1)),
    has_attachments         INTEGER NOT NULL DEFAULT 0 CHECK(has_attachments IN (0, 1)),
    importance              TEXT NOT NULL DEFAULT 'normal'
        CHECK(importance IN ('low', 'normal', 'high')),
    folder_id               TEXT,
    parent_folder_name      TEXT,
    sync_status             TEXT NOT NULL DEFAULT 'synced'
        CHECK(sync_status IN ('synced', 'pending_local', 'deleted_remote')),
    local_updated_at        INTEGER NOT NULL,
    remote_updated_at       INTEGER,
    PRIMARY KEY (account_id, id),
    UNIQUE (account_id, folder_id, remote_key),
    FOREIGN KEY (account_id, session_id) REFERENCES sessions(account_id, id)
);

CREATE INDEX idx_messages_session
    ON messages(account_id, session_id, received_at DESC);

CREATE INDEX idx_messages_account_received
    ON messages(account_id, received_at DESC);

CREATE INDEX idx_messages_internet_message_id
    ON messages(account_id, internet_message_id);

CREATE INDEX idx_messages_in_reply_to
    ON messages(account_id, in_reply_to);

CREATE INDEX idx_messages_folder
    ON messages(account_id, folder_id);

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

CREATE TABLE session_aggregate_jobs (
    message_id   TEXT NOT NULL,
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    generation   INTEGER NOT NULL DEFAULT 0,
    outcome      TEXT CHECK(outcome IS NULL OR outcome IN ('completed', 'partial', 'deferred', 'skipped')),
    status       TEXT NOT NULL CHECK(status IN ('pending', 'processing', 'completed', 'retry', 'failed')),
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

CREATE INDEX idx_session_aggregate_jobs_poll
    ON session_aggregate_jobs(status, available_at);

CREATE INDEX idx_session_aggregate_jobs_lease
    ON session_aggregate_jobs(lease_expires_at)
    WHERE status = 'processing';

CREATE TABLE pending_message_mutations (
    account_id      TEXT NOT NULL,
    message_id      TEXT NOT NULL,
    kind            TEXT NOT NULL CHECK(kind IN ('read', 'delete')),
    target_folder_id TEXT NOT NULL,
    target_remote_key TEXT NOT NULL,
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK(is_active IN (0, 1)),
    desired_is_read INTEGER CHECK(desired_is_read IS NULL OR desired_is_read IN (0, 1)),
    version         INTEGER NOT NULL DEFAULT 1 CHECK(version >= 1),
    attempts        INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
    available_at    INTEGER NOT NULL,
    last_error      TEXT,
    PRIMARY KEY (account_id, message_id),
    FOREIGN KEY (account_id, message_id) REFERENCES messages(account_id, id) ON DELETE CASCADE,
    CHECK (
        (kind = 'read' AND desired_is_read IS NOT NULL)
        OR (kind = 'delete' AND desired_is_read IS NULL)
    )
);

CREATE INDEX idx_pending_message_mutations_due
    ON pending_message_mutations(account_id, available_at);

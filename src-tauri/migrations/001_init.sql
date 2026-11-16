-- V1 schema（对齐 .docs/V1.0.0/data-model.md；body_content 可空表示未加载）

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
    show_group_messages_in_contact_conversation
        INTEGER NOT NULL DEFAULT 0
        CHECK(show_group_messages_in_contact_conversation IN (0, 1)),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    UNIQUE (email)
);

CREATE INDEX idx_accounts_email ON accounts(email);

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

CREATE TABLE contacts (
    id                  TEXT NOT NULL,
    account_id          TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    graph_id            TEXT,
    display_name        TEXT NOT NULL DEFAULT '',
    normalized_address  TEXT NOT NULL COLLATE NOCASE,
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL,
    PRIMARY KEY (account_id, id),
    UNIQUE (account_id, graph_id)
);

CREATE UNIQUE INDEX idx_contacts_account_address
    ON contacts(account_id, normalized_address);

CREATE INDEX idx_contacts_account ON contacts(account_id);

CREATE TABLE messages (
    id                      TEXT NOT NULL,
    account_id              TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    graph_id                TEXT,
    thread_id               TEXT CHECK(thread_id IS NULL OR length(trim(thread_id)) > 0),
    graph_change_key        TEXT,
    internet_message_id     TEXT,
    conversation_index      BLOB,
    subject                 TEXT NOT NULL DEFAULT '',
    body_preview            TEXT NOT NULL DEFAULT '',
    body_content_type       TEXT NOT NULL DEFAULT 'text'
        CHECK(body_content_type IN ('text', 'html')),
    -- NULL = 未加载；空串 = 已加载且正文为空
    body_content            TEXT,
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
    folder_id               TEXT,
    parent_folder_name      TEXT,
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

CREATE TABLE groups (
    id              TEXT NOT NULL,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name            TEXT NOT NULL DEFAULT '',
    name_is_manual  INTEGER NOT NULL DEFAULT 0 CHECK(name_is_manual IN (0, 1)),
    source          TEXT NOT NULL CHECK(source IN ('auto', 'manual')),
    send_members_is_manual INTEGER NOT NULL DEFAULT 0 CHECK(send_members_is_manual IN (0, 1)),
    merged_into_id  TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    PRIMARY KEY (account_id, id),
    FOREIGN KEY (account_id, merged_into_id) REFERENCES groups(account_id, id),
    CHECK (merged_into_id IS NULL OR merged_into_id != id)
);

CREATE INDEX idx_groups_account ON groups(account_id);
CREATE INDEX idx_groups_merged_into ON groups(account_id, merged_into_id);

CREATE TABLE group_thread (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    group_id   TEXT NOT NULL,
    thread_id  TEXT NOT NULL,
    linked_at  INTEGER NOT NULL,
    PRIMARY KEY (account_id, thread_id),
    FOREIGN KEY (account_id, group_id) REFERENCES groups(account_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_group_thread_group ON group_thread(account_id, group_id);

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

CREATE TABLE conversations (
    id                 TEXT NOT NULL,
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
    PRIMARY KEY (account_id, id),
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

CREATE TABLE conversation_messages (
    account_id      TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    message_id      TEXT NOT NULL,
    attached_at     INTEGER NOT NULL,
    PRIMARY KEY (account_id, conversation_id, message_id),
    FOREIGN KEY (account_id, conversation_id) REFERENCES conversations(account_id, id) ON DELETE CASCADE,
    FOREIGN KEY (account_id, message_id) REFERENCES messages(account_id, id) ON DELETE CASCADE
);

CREATE INDEX idx_conversation_messages_message
    ON conversation_messages(account_id, message_id);

CREATE TABLE conversation_aggregate_jobs (
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

CREATE INDEX idx_conversation_aggregate_jobs_poll
    ON conversation_aggregate_jobs(status, available_at);

CREATE INDEX idx_conversation_aggregate_jobs_lease
    ON conversation_aggregate_jobs(lease_expires_at)
    WHERE status = 'processing';

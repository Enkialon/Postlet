-- 本地字段待推远端（已读 / 删除）；版本用于确认后乱序保护。
CREATE TABLE pending_message_mutations (
    account_id      TEXT NOT NULL,
    message_id      TEXT NOT NULL,
    kind            TEXT NOT NULL CHECK(kind IN ('read', 'delete')),
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

-- Graph Delta Remove 待核对：事务外 locate，revision 匹配才生效。
CREATE TABLE message_locate_pending (
    account_id        TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    graph_id          TEXT NOT NULL,
    source_folder_id  TEXT NOT NULL,
    revision          INTEGER NOT NULL DEFAULT 1 CHECK(revision >= 1),
    available_at      INTEGER NOT NULL,
    last_error        TEXT,
    updated_at        INTEGER NOT NULL,
    PRIMARY KEY (account_id, graph_id)
);

CREATE INDEX idx_message_locate_pending_due
    ON message_locate_pending(account_id, available_at);

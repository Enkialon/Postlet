-- 账号邮件协议配置：协议类型与端点分离于服务商品牌；凭证仍落 credentials 目录。
CREATE TABLE account_mail_transport (
    account_id          TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    receive_protocol    TEXT NOT NULL DEFAULT 'graph'
        CHECK(receive_protocol IN ('graph', 'imap')),
    send_protocol       TEXT NOT NULL DEFAULT 'graph'
        CHECK(send_protocol IN ('graph', 'smtp')),
    auth_kind           TEXT NOT NULL DEFAULT 'oauth2'
        CHECK(auth_kind IN ('password', 'oauth2')),
    imap_host           TEXT,
    imap_port           INTEGER,
    imap_tls            TEXT
        CHECK(imap_tls IS NULL OR imap_tls IN ('implicit', 'starttls', 'plain')),
    smtp_host           TEXT,
    smtp_port           INTEGER,
    smtp_tls            TEXT
        CHECK(smtp_tls IS NULL OR smtp_tls IN ('implicit', 'starttls', 'plain')),
    username            TEXT,
    -- IMAP IDLE 能力探测结果；NULL=未探测
    imap_idle_supported INTEGER
        CHECK(imap_idle_supported IS NULL OR imap_idle_supported IN (0, 1)),
    updated_at          INTEGER NOT NULL
);

-- 既有 Outlook 账号默认 Graph + OAuth2
INSERT INTO account_mail_transport (
    account_id, receive_protocol, send_protocol, auth_kind, updated_at
)
SELECT id, 'graph', 'graph', 'oauth2', updated_at FROM accounts;

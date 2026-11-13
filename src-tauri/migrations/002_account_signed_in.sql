-- 登录会话标记：退出只清凭证并置 0，保留账户行与邮件数据；再登录置 1 并复用原 AccountId。
ALTER TABLE accounts ADD COLUMN is_signed_in INTEGER NOT NULL DEFAULT 1
    CHECK(is_signed_in IN (0, 1));

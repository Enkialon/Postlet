-- 是否参与「全部账号」列表查询；与 is_signed_in 独立：可保持登录但不纳入跨账号扫描。
ALTER TABLE accounts ADD COLUMN include_in_all INTEGER NOT NULL DEFAULT 1
    CHECK(include_in_all IN (0, 1));

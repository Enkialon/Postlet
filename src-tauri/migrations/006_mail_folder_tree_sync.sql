-- 文件夹树 + 是否纳入同步（直接挂在同步状态表上）
ALTER TABLE mail_folder_sync_state ADD COLUMN parent_folder_id TEXT;
ALTER TABLE mail_folder_sync_state ADD COLUMN display_name TEXT NOT NULL DEFAULT '';
ALTER TABLE mail_folder_sync_state ADD COLUMN sync_enabled INTEGER NOT NULL DEFAULT 0
    CHECK(sync_enabled IN (0, 1));

-- 已有收件箱行默认开启同步
UPDATE mail_folder_sync_state
SET sync_enabled = 1,
    display_name = CASE
        WHEN display_name = '' AND lower(folder_id) IN ('inbox') THEN 'Inbox'
        ELSE display_name
    END
WHERE lower(folder_id) IN ('inbox');

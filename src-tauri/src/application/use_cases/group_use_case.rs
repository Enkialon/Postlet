//! 群组用例
use crate::infrastructure::repository::groups;
use sqlx::SqlitePool;

use crate::application::dto::{
    CreateManualGroupCommand, CursorPage, GroupListQuery, MergeGroupsCommand,
    MoveThreadToGroupCommand, RenameGroupCommand, UpdateGroupSendMembersCommand,
};
use crate::application::error::AppError;
use crate::domain::entities::{Group, GroupParticipant};
use crate::domain::value_objects::{AccountId, GroupId};

/// 分页读取账户内的业务群组，默认排除已合并到其他群组的记录。
///
/// 是否包含已合并群组由查询参数 `include_merged` 决定。
pub async fn list_groups(
    db: &SqlitePool,
    query: GroupListQuery,
) -> Result<CursorPage<Group>, AppError> {
    let mut conn = db.acquire().await?;
    groups::list(&mut conn, &query).await.map_err(Into::into)
}

/// 在指定账户内按本地 ID 查询群组；不存在时返回未找到错误。
///
/// 此方法不自动沿合并关系跳转到存活根群组。
pub async fn get_group(
    db: &SqlitePool,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Group, AppError> {
    let mut conn = db.acquire().await?;
    groups::get(&mut conn, account_id, group_id)
        .await?
        .ok_or_else(|| AppError::not_found(format!("群组不存在: {group_id}")))
}

/// 查询指定账户和群组的成员投影，保留往来观测成员与发信成员标识。
pub async fn list_participants(
    db: &SqlitePool,
    account_id: &AccountId,
    group_id: &GroupId,
) -> Result<Vec<GroupParticipant>, AppError> {
    let mut conn = db.acquire().await?;
    groups::list_participants(&mut conn, account_id, group_id)
        .await
        .map_err(Into::into)
}

/// 创建用户手动命名的群组，并保存命令中的发信成员集合。
///
/// 群组与成员须在同一事务写入；当前为骨架，返回未实现错误。
pub async fn create_manual_group(
    db: &SqlitePool,
    cmd: CreateManualGroupCommand,
) -> Result<Group, AppError> {
    let _ = (db, cmd);
    Err(AppError::not_implemented(
        "group_use_case::create_manual_group",
    ))
}

/// 修改群组名称并标记为人工命名，避免后续自动聚合覆盖用户名称。
///
/// 当前为骨架，返回未实现错误，不修改群组。
pub async fn rename_group(db: &SqlitePool, cmd: RenameGroupCommand) -> Result<Group, AppError> {
    let _ = (db, cmd);
    Err(AppError::not_implemented("group_use_case::rename_group"))
}

/// 将源群组并入同账户的目标根群组，并返回存活群组。
///
/// 实现时须校验合并无环，并原子迁移线程、邮件附着及相关投影。
/// 当前为骨架，返回未实现错误。
pub async fn merge_groups(db: &SqlitePool, cmd: MergeGroupsCommand) -> Result<Group, AppError> {
    let _ = (db, cmd);
    Err(AppError::not_implemented("group_use_case::merge_groups"))
}

/// 将指定线程的全部邮件迁移到同账户的目标根群组。
///
/// 主归属、邮件附着及两侧会话和成员投影须在同一事务更新。
/// 当前为骨架，返回未实现错误。
pub async fn move_thread_to_group(
    db: &SqlitePool,
    cmd: MoveThreadToGroupCommand,
) -> Result<(), AppError> {
    let _ = (db, cmd);
    Err(AppError::not_implemented(
        "group_use_case::move_thread_to_group",
    ))
}

/// 替换群组的发信成员集合，并按命令设置是否锁定为人工配置。
///
/// 不以此覆盖往来观测成员；当前为骨架，返回未实现错误。
pub async fn update_send_members(
    db: &SqlitePool,
    cmd: UpdateGroupSendMembersCommand,
) -> Result<Vec<GroupParticipant>, AppError> {
    let _ = (db, cmd);
    Err(AppError::not_implemented(
        "group_use_case::update_send_members",
    ))
}

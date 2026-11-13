//! 聚合后台用例：由同步完成或定时触发；不直接暴露给 UI
use sqlx::SqlitePool;

use crate::application::error::AppError;

/// 处理至多 `limit` 条可用聚合任务，返回成功完成数。
///
/// 执行时须校验任务代次和租约，再读取上下文、调用领域纯函数并更新会话投影。
/// 聚合结果与任务完成须共用事务，避免旧任务覆盖新状态。
/// 当前为骨架，返回未实现错误，不领取或完成任务。
pub async fn process_available_jobs(db: &SqlitePool, limit: u32) -> Result<u32, AppError> {
    let _ = (db, limit);
    Err(AppError::not_implemented(
        "aggregate_worker_use_case::process_available_jobs",
    ))
}

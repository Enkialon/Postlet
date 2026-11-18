//! 所有函数借用调用方连接，不自行提交事务。
pub mod accounts;
pub mod archive_tasks;
pub mod attachments;
pub mod contacts;
pub mod download_candidates;
pub mod folder_states;
pub mod groups;
pub mod jobs;
pub mod mail_transports;
pub mod messages;
pub mod mutations;
pub mod outbound_snapshots;
pub mod send_tasks;
pub mod sessions;
mod rows;

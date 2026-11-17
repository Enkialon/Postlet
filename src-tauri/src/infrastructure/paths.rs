//! 应用目录布局：配置与本地邮件数据分离。
//!
//! ```text
//! app_config_dir/                 # 小配置（可漫游）
//!   settings.json                 # 预留
//!
//! app_local_data_dir/             # 机器本地大数据
//!   data/postlet.db               # 邮件 / 会话库
//!   attachments/{account_id}/     # 受管理附件（tmp / objects / lock）
//!   credentials/                  # 机密（预留；勿明文长期存放）
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

/// 已解析的应用目录根与常用路径。
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// 软件配置根（`app_config_dir`）
    pub config_dir: PathBuf,
    /// 本地数据根（`app_local_data_dir`）
    pub local_data_dir: PathBuf,
    /// `local_data_dir/data`
    pub data_dir: PathBuf,
    /// `local_data_dir/attachments`
    pub attachments_dir: PathBuf,
    /// `local_data_dir/credentials`
    pub credentials_dir: PathBuf,
    /// `data_dir/postlet.db`
    pub database_path: PathBuf,
}

impl AppPaths {
    /// 从 Tauri PathResolver 解析并创建目录；必要时从旧 `app_data_dir/postlet.db` 迁到新路径。
    pub fn resolve<R: Runtime>(app: &AppHandle<R>) -> Result<Self, String> {
        let resolver = app.path();
        let config_dir = resolver
            .app_config_dir()
            .map_err(|e| format!("无法解析 app_config_dir: {e}"))?;
        let local_data_dir = resolver
            .app_local_data_dir()
            .map_err(|e| format!("无法解析 app_local_data_dir: {e}"))?;

        let data_dir = local_data_dir.join("data");
        let attachments_dir = local_data_dir.join("attachments");
        let credentials_dir = local_data_dir.join("credentials");
        let database_path = data_dir.join("postlet.db");

        for dir in [
            &config_dir,
            &local_data_dir,
            &data_dir,
            &attachments_dir,
            &credentials_dir,
        ] {
            fs::create_dir_all(dir).map_err(|e| format!("无法创建目录 {}: {e}", dir.display()))?;
        }

        // 兼容上一版：根目录下的 postlet.db → data/postlet.db
        if let Ok(legacy_root) = resolver.app_data_dir() {
            migrate_legacy_database(&legacy_root.join("postlet.db"), &database_path)?;
        }

        Ok(Self {
            config_dir,
            local_data_dir,
            data_dir,
            attachments_dir,
            credentials_dir,
            database_path,
        })
    }
}

fn migrate_legacy_database(legacy: &Path, target: &Path) -> Result<(), String> {
    if !legacy.exists() || target.exists() || legacy == target {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建数据目录: {e}"))?;
    }
    fs::rename(legacy, target).or_else(|_| {
        fs::copy(legacy, target)
            .map(|_| ())
            .and_then(|_| fs::remove_file(legacy))
    })
    .map_err(|e| format!("迁移旧数据库失败 {} → {}: {e}", legacy.display(), target.display()))?;

    // WAL 旁路文件一并迁移（若存在）
    for suffix in ["-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", legacy.display()));
        let to = PathBuf::from(format!("{}{suffix}", target.display()));
        if from.exists() && !to.exists() {
            let _ = fs::rename(&from, &to).or_else(|_| {
                fs::copy(&from, &to)
                    .map(|_| ())
                    .and_then(|_| fs::remove_file(&from))
            });
        }
    }
    Ok(())
}

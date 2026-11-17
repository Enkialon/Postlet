//! 受管理附件文件系统：临时写入、校验、原子安装、受控读取与删除。
//!
//! 布局：
//! ```text
//! attachments/{account_id}/
//!   attachment.lock
//!   tmp/{uuid}.part
//!   objects/{storage_key}
//! ```

use crate::domain::value_objects::AccountId;
use crate::infrastructure::error::InfraError;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// 账号级附件目录锁；Drop 时自动释放（进程内与进程间共用同一锁文件）。
pub struct AttachmentLock {
    _file: File,
}

/// 账号附件根目录：`attachments/{account_id}`
pub fn account_dir(attachments_root: &Path, account_id: &AccountId) -> PathBuf {
    attachments_root.join(account_id.as_str())
}

fn objects_dir(account_root: &Path) -> PathBuf {
    account_root.join("objects")
}

fn tmp_dir(account_root: &Path) -> PathBuf {
    account_root.join("tmp")
}

fn ensure_layout(account_root: &Path) -> Result<(), InfraError> {
    fs::create_dir_all(objects_dir(account_root)).map_err(|_| InfraError::Storage {
        message: "创建附件对象目录失败".into(),
    })?;
    fs::create_dir_all(tmp_dir(account_root)).map_err(|_| InfraError::Storage {
        message: "创建附件暂存目录失败".into(),
    })?;
    Ok(())
}

/// 获取账号附件锁；忙时返回可映射为可重试冲突的 `InfraError::Conflict`。
pub fn lock_account(
    attachments_root: &Path,
    account_id: &AccountId,
) -> Result<AttachmentLock, InfraError> {
    let root = account_dir(attachments_root, account_id);
    ensure_layout(&root)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("attachment.lock"))
        .map_err(|_| InfraError::Storage {
            message: "打开附件锁失败".into(),
        })?;
    file.try_lock().map_err(|_| InfraError::Conflict {
        message: "附件存储繁忙，请稍后重试".into(),
    })?;
    Ok(AttachmentLock { _file: file })
}

/// 写入临时文件并计算 SHA-256；返回临时路径与摘要（hex）。
pub fn write_temp(
    attachments_root: &Path,
    account_id: &AccountId,
    bytes: &[u8],
) -> Result<(PathBuf, String), InfraError> {
    let root = account_dir(attachments_root, account_id);
    ensure_layout(&root)?;
    let path = tmp_dir(&root).join(format!("{}.part", Uuid::new_v4()));
    let mut file = File::create(&path).map_err(|_| InfraError::Storage {
        message: "创建附件临时文件失败".into(),
    })?;
    file.write_all(bytes).map_err(|_| InfraError::Storage {
        message: "写入附件临时文件失败".into(),
    })?;
    file.sync_all().map_err(|_| InfraError::Storage {
        message: "持久化附件临时文件失败".into(),
    })?;
    let digest = hex::encode(Sha256::digest(bytes));
    Ok((path, digest))
}

/// 校验临时文件长度与摘要后，原子安装到 `objects/{storage_key}`。
pub fn install_from_temp(
    attachments_root: &Path,
    account_id: &AccountId,
    temp_path: &Path,
    storage_key: &str,
    expected_len: u64,
    expected_digest: &str,
) -> Result<PathBuf, InfraError> {
    if storage_key.is_empty()
        || storage_key.contains('/')
        || storage_key.contains('\\')
        || storage_key.contains("..")
    {
        return Err(InfraError::Storage {
            message: "非法附件存储键".into(),
        });
    }
    let meta = fs::metadata(temp_path).map_err(|_| InfraError::Storage {
        message: "读取附件临时文件失败".into(),
    })?;
    if meta.len() != expected_len {
        let _ = fs::remove_file(temp_path);
        return Err(InfraError::Storage {
            message: "附件长度校验失败".into(),
        });
    }
    let mut file = File::open(temp_path).map_err(|_| InfraError::Storage {
        message: "打开附件临时文件失败".into(),
    })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|_| InfraError::Storage {
            message: "读取附件临时文件失败".into(),
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hex::encode(hasher.finalize());
    if digest != expected_digest {
        let _ = fs::remove_file(temp_path);
        return Err(InfraError::Storage {
            message: "附件内容摘要校验失败".into(),
        });
    }

    let root = account_dir(attachments_root, account_id);
    ensure_layout(&root)?;
    let final_path = objects_dir(&root).join(storage_key);
    if final_path.exists() {
        let _ = fs::remove_file(temp_path);
        return Err(InfraError::Storage {
            message: "附件内容已存在，不可覆盖".into(),
        });
    }
    fs::rename(temp_path, &final_path).map_err(|_| InfraError::Storage {
        message: "安装附件内容失败".into(),
    })?;
    // 尽力 fsync 目录项（平台差异可忽略）
    if let Ok(dir) = File::open(objects_dir(&root)) {
        let _ = dir.sync_all();
    }
    Ok(final_path)
}

/// 受控读取已安装内容。
pub fn load_bytes(
    attachments_root: &Path,
    account_id: &AccountId,
    storage_key: &str,
) -> Result<Vec<u8>, InfraError> {
    let path = objects_dir(&account_dir(attachments_root, account_id)).join(storage_key);
    fs::read(&path).map_err(|_| InfraError::Storage {
        message: format!("附件内容缺失或不可读: {storage_key}"),
    })
}

/// 删除已安装对象；文件不存在视为成功。
pub fn delete_object(
    attachments_root: &Path,
    account_id: &AccountId,
    storage_key: &str,
) -> Result<(), InfraError> {
    let path = objects_dir(&account_dir(attachments_root, account_id)).join(storage_key);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(InfraError::Storage {
            message: format!("删除附件文件失败: {storage_key}"),
        }),
    }
}

/// 清理暂存目录中残留的 `.part` 文件。
pub fn clean_temp_dir(attachments_root: &Path, account_id: &AccountId) -> Result<(), InfraError> {
    let dir = tmp_dir(&account_dir(attachments_root, account_id));
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&dir).map_err(|_| InfraError::Storage {
        message: "读取附件暂存目录失败".into(),
    })? {
        let entry = entry.map_err(|_| InfraError::Storage {
            message: "遍历附件暂存目录失败".into(),
        })?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("part") {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

/// 列出 objects 目录下的 storage_key。
pub fn list_object_keys(
    attachments_root: &Path,
    account_id: &AccountId,
) -> Result<Vec<String>, InfraError> {
    let dir = objects_dir(&account_dir(attachments_root, account_id));
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut keys = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|_| InfraError::Storage {
        message: "读取附件对象目录失败".into(),
    })? {
        let entry = entry.map_err(|_| InfraError::Storage {
            message: "遍历附件对象目录失败".into(),
        })?;
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            if let Some(name) = entry.file_name().to_str() {
                keys.push(name.to_string());
            }
        }
    }
    Ok(keys)
}

/// 对象文件是否存在。
pub fn object_exists(
    attachments_root: &Path,
    account_id: &AccountId,
    storage_key: &str,
) -> bool {
    objects_dir(&account_dir(attachments_root, account_id))
        .join(storage_key)
        .is_file()
}

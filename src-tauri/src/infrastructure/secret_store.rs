//! 系统钥匙串保护加密密钥，普通磁盘仅保存认证加密的密文。
//! 使用加密文件以避免系统凭据项对大型 OAuth token 的长度限制。

use crate::application::error::PortError;
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn failure(message: &str) -> PortError {
    PortError::Storage {
        message: message.into(),
    }
}

fn cipher(dir: &Path, create: bool) -> Result<Aes256Gcm, PortError> {
    fs::create_dir_all(dir).map_err(|_| failure("创建凭据目录失败"))?;
    let canonical = fs::canonicalize(dir).map_err(|_| failure("解析凭据目录失败"))?;
    // 跨进程串行首次密钥创建，防止两个实例各自生成密钥后覆盖。
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("key.lock"))
        .map_err(|_| failure("打开凭据锁失败"))?;
    lock.lock().map_err(|_| failure("锁定凭据密钥失败"))?;
    let entry = keyring::Entry::new("com.postlet.credentials", &canonical.to_string_lossy())
        .map_err(|_| failure("系统凭据存储不可用"))?;
    let encoded = match entry.get_password() {
        Ok(value) => value,
        Err(keyring::Error::NoEntry) => {
            let has_ciphertext = fs::read_dir(dir)
                .map_err(|_| failure("检查凭据目录失败"))?
                .try_fold(false, |found, entry| {
                    entry.map(|e| found || e.path().extension().is_some_and(|ext| ext == "secret"))
                })
                .map_err(|_| failure("检查凭据目录失败"))?;
            if !create || has_ciphertext {
                return Err(failure("系统凭据密钥缺失，请清理失效凭据后重新登录"));
            }
            let key = Aes256Gcm::generate_key(&mut OsRng);
            let encoded = STANDARD.encode(key);
            entry
                .set_password(&encoded)
                .map_err(|_| failure("保存系统凭据密钥失败"))?;
            encoded
        }
        Err(_) => return Err(failure("读取系统凭据密钥失败，请解锁系统钥匙串")),
    };
    let key = STANDARD
        .decode(encoded)
        .map_err(|_| failure("系统凭据密钥无效"))?;
    Aes256Gcm::new_from_slice(&key).map_err(|_| failure("系统凭据密钥无效"))
}

fn path(dir: &Path, kind: &str, id: &str) -> PathBuf {
    let encoded: String = id.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    dir.join(format!("{kind}-{encoded}.secret"))
}

fn encrypt(cipher: &Aes256Gcm, raw: &[u8], identity: &[u8]) -> Result<Vec<u8>, PortError> {
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let encrypted = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: raw,
                aad: identity,
            },
        )
        .map_err(|_| failure("加密凭据失败"))?;
    let mut result = nonce.to_vec();
    result.extend(encrypted);
    Ok(result)
}

fn decrypt(cipher: &Aes256Gcm, encrypted: &[u8], identity: &[u8]) -> Result<Vec<u8>, PortError> {
    if encrypted.len() < 12 {
        return Err(failure("本地凭据损坏，请重新登录"));
    }
    cipher
        .decrypt(
            Nonce::from_slice(&encrypted[..12]),
            Payload {
                msg: &encrypted[12..],
                aad: identity,
            },
        )
        .map_err(|_| failure("本地凭据校验失败，请重新登录"))
}

fn replace(path: &Path, raw: &[u8]) -> Result<(), PortError> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())
        .map_err(|_| failure("创建凭据临时文件失败"))?;
    file.write_all(raw).map_err(|_| failure("写入凭据失败"))?;
    file.as_file()
        .sync_all()
        .map_err(|_| failure("持久化凭据失败"))?;
    file.persist(path)
        .map_err(|_| failure("原子替换凭据失败"))?;
    #[cfg(unix)]
    fs::File::open(path.parent().unwrap())
        .and_then(|dir| dir.sync_all())
        .map_err(|_| failure("持久化凭据目录失败"))?;
    Ok(())
}

pub fn save<T: Serialize>(dir: &Path, kind: &str, id: &str, value: &T) -> Result<(), PortError> {
    let cipher = cipher(dir, true)?;
    let raw = serde_json::to_vec(value).map_err(|_| failure("序列化凭据失败"))?;
    let identity = format!("{kind}:{id}");
    replace(
        &path(dir, kind, id),
        &encrypt(&cipher, &raw, identity.as_bytes())?,
    )
}

pub fn load<T: DeserializeOwned>(dir: &Path, kind: &str, id: &str) -> Result<Option<T>, PortError> {
    let raw = match fs::read(path(dir, kind, id)) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(failure("读取凭据失败")),
    };
    let identity = format!("{kind}:{id}");
    let raw = decrypt(&cipher(dir, false)?, &raw, identity.as_bytes())?;
    serde_json::from_slice(&raw)
        .map(Some)
        .map_err(|_| failure("本地凭据格式无效"))
}

pub fn delete(dir: &Path, kind: &str, id: &str) -> Result<(), PortError> {
    match fs::remove_file(path(dir, kind, id)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(failure("删除凭据失败，请重试退出")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ciphertext_detects_tampering_and_wrong_account() {
        let cipher = Aes256Gcm::new(&Aes256Gcm::generate_key(&mut OsRng));
        let secret = b"password and refresh token";
        let encrypted = encrypt(&cipher, secret, b"mail:account-1").unwrap();
        assert!(!encrypted.windows(secret.len()).any(|bytes| bytes == secret));
        assert_eq!(
            decrypt(&cipher, &encrypted, b"mail:account-1").unwrap(),
            secret
        );
        assert!(decrypt(&cipher, &encrypted, b"mail:account-2").is_err());
        assert!(decrypt(&cipher, &encrypted, b"graph:account-1").is_err());
        let mut damaged = encrypted.clone();
        damaged[12] ^= 1;
        assert!(decrypt(&cipher, &damaged, b"mail:account-1").is_err());
        assert!(decrypt(&cipher, &encrypted[..5], b"mail:account-1").is_err());
    }

    #[test]
    fn atomic_replace_and_failed_replace_preserve_committed_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("test.secret");
        replace(&target, b"first").unwrap();
        replace(&target, b"second").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"second");
        // A failed temporary write cannot touch the committed target.
        assert!(replace(&dir.path().join("missing/test.secret"), b"third").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"second");
    }

    #[test]
    #[ignore = "writes an isolated temporary OS keyring entry"]
    fn native_keyring_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let result = (|| -> Result<(), PortError> {
            let value = "large-secret-token".repeat(1000);
            save(dir.path(), "graph", "test-account", &value)?;
            assert_eq!(
                load::<String>(dir.path(), "graph", "test-account")?,
                Some(value.clone())
            );
            let raw = fs::read(path(dir.path(), "graph", "test-account")).unwrap();
            assert!(
                !raw.windows(18)
                    .any(|window| window == b"large-secret-token")
            );
            save(dir.path(), "graph", "test-account", &"replacement")?;
            assert_eq!(
                load::<String>(dir.path(), "graph", "test-account")?,
                Some("replacement".into())
            );
            delete(dir.path(), "graph", "test-account")?;
            assert!(load::<String>(dir.path(), "graph", "test-account")?.is_none());
            delete(dir.path(), "graph", "test-account")?;
            Ok(())
        })();
        let canonical = fs::canonicalize(dir.path()).unwrap();
        let entry =
            keyring::Entry::new("com.postlet.credentials", &canonical.to_string_lossy()).unwrap();
        let cleanup = entry.delete_credential();
        result.unwrap();
        cleanup.unwrap();
    }
}

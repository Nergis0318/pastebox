use crate::config::Config;
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

const ID_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const ID_LENGTH: usize = 5;
const MAX_ID_RETRIES: usize = 100;

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub id: String,
    pub password_hash: Option<String>,
    pub delete_token_hash: String,
    pub created_at: String,
    pub expires_at: String,
    pub data_policy: String,
    pub size: u64,
    pub content_type: String,
}

pub struct PasteStore {
    data_dir: PathBuf,
    ttl_days: u32,
}

impl PasteStore {
    pub fn new(config: &Config) -> io::Result<Self> {
        fs::create_dir_all(&config.data_dir)?;
        Ok(Self {
            data_dir: config.data_dir.clone(),
            ttl_days: config.expire_days,
        })
    }

    pub fn create(
        &self,
        content: &[u8],
        content_type: &str,
        password: Option<&str>,
        delete_token: &str,
        data_policy: &str,
    ) -> io::Result<Metadata> {
        let id = self.reserve_path()?;
        let path = self.paste_path(&id);

        let now = Utc::now();

        let mut file = fs::File::create(&path)?;
        file.write_all(content)?;
        file.sync_all()?;

        let expires_at = if data_policy == "permanent" {
            "never".to_string()
        } else {
            (now + chrono::Duration::days(self.ttl_days as i64))
                .format("%Y-%m-%dT%H:%M:%S%.9fZ")
                .to_string()
        };

        let metadata = Metadata {
            id: id.clone(),
            password_hash: password.map(|p| hex::encode(Sha256::digest(p.as_bytes()))),
            delete_token_hash: hex::encode(Sha256::digest(delete_token.as_bytes())),
            created_at: now.format("%Y-%m-%dT%H:%M:%S%.9fZ").to_string(),
            expires_at,
            data_policy: data_policy.to_string(),
            size: content.len() as u64,
            content_type: content_type.to_string(),
        };

        let meta_path = self.meta_path(&id);
        let json = serde_json::to_string_pretty(&metadata)?;
        fs::write(&meta_path, json)?;

        tracing::info!(
            id = %id,
            size = content.len(),
            content_type = %content_type,
            data_policy = %data_policy,
            protected = password.is_some(),
            "paste created"
        );

        Ok(metadata)
    }

    pub fn open(&self, id: &str) -> Result<(Metadata, Vec<u8>), crate::errors::AppError> {
        use crate::errors::AppError;
        if !valid_id(id) {
            return Err(AppError::NotFound);
        }

        let meta_path = self.meta_path(id);
        let meta: Metadata = serde_json::from_str(
            &fs::read_to_string(&meta_path)
                .map_err(|_| AppError::NotFound)?
        )
        .map_err(|_| AppError::NotFound)?;

        if let Ok(expires) = DateTime::parse_from_rfc3339(&meta.expires_at)
            && Utc::now() > expires.with_timezone(&Utc)
        {
            return Err(AppError::Gone);
        }

        let content = fs::read(self.paste_path(id))
            .map_err(|_| AppError::NotFound)?;

        Ok((meta, content))
    }

    pub fn delete(&self, id: &str, delete_token: &str) -> Result<(), crate::errors::AppError> {
        use crate::errors::AppError;
        let meta: Metadata =
            serde_json::from_str(
                &fs::read_to_string(self.meta_path(id))
                    .map_err(|_| AppError::NotFound)?
            )
            .map_err(|_| AppError::NotFound)?;

        let token_hash = hex::encode(Sha256::digest(delete_token.as_bytes()));
        if token_hash != meta.delete_token_hash {
            return Err(AppError::Forbidden);
        }

        let _ = fs::remove_file(self.paste_path(id));
        let _ = fs::remove_file(self.meta_path(id));

        tracing::info!(id = %id, "paste deleted");
        Ok(())
    }

    pub fn admin_delete(&self, id: &str) -> Result<(), crate::errors::AppError> {
        use crate::errors::AppError;
        let meta_path = self.meta_path(id);
        if !meta_path.exists() {
            return Err(AppError::NotFound);
        }
        let _ = fs::remove_file(self.paste_path(id));
        let _ = fs::remove_file(&meta_path);
        tracing::info!(id = %id, "paste deleted by admin");
        Ok(())
    }

    pub fn list_pastes(&self) -> io::Result<Vec<Metadata>> {
        let mut pastes = Vec::new();
        for entry in fs::read_dir(&self.data_dir)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().ends_with(".json")
                && let Ok(meta) = serde_json::from_reader::<_, Metadata>(
                    fs::File::open(entry.path())?
                )
            {
                pastes.push(meta);
            }
        }
        pastes.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(pastes)
    }

    pub fn cleanup_expired(&self) -> io::Result<usize> {
        let mut removed = 0;
        for entry in fs::read_dir(&self.data_dir)? {
            let entry = entry?;
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".json") {
                let id = &fname[..fname.len() - 5];
                let meta: Metadata = match fs::read_to_string(entry.path()) {
                    Ok(s) => match serde_json::from_str(&s) {
                        Ok(m) => m,
                        Err(_) => continue,
                    },
                    Err(_) => continue,
                };
                if meta.data_policy == "permanent" {
                    continue;
                }
                if let Ok(expires) = DateTime::parse_from_rfc3339(&meta.expires_at)
                    && Utc::now() > expires.with_timezone(&Utc)
                {
                    let _ = fs::remove_file(self.paste_path(id));
                    let _ = fs::remove_file(self.meta_path(id));
                    tracing::info!(id = %id, "expired paste removed");
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            tracing::info!(removed, "cleanup complete");
        }
        Ok(removed)
    }

    pub fn meta_path(&self, id: &str) -> PathBuf {
        self.data_dir.join(format!("{id}.json"))
    }

    pub fn paste_path(&self, id: &str) -> PathBuf {
        self.data_dir.join(id)
    }

    fn reserve_path(&self) -> io::Result<String> {
        let mut rng = rand::thread_rng();
        for _ in 0..MAX_ID_RETRIES {
            let id: String = (0..ID_LENGTH)
                .map(|_| {
                    let idx = rng.gen_range(0..ID_ALPHABET.len());
                    ID_ALPHABET[idx] as char
                })
                .collect();
            if !self.meta_path(&id).exists() {
                return Ok(id);
            }
        }
        Err(io::Error::other("failed to generate unique ID"))
    }
}

pub fn valid_id(s: &str) -> bool {
    s.len() == ID_LENGTH && s.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_id() {
        assert!(valid_id("AbC12"));
        assert!(!valid_id(""));
        assert!(!valid_id("AbC12!"));
        assert!(!valid_id("AbCdEf"));
    }

    #[test]
    fn test_paste_create_and_open_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            listen_addr: "0.0.0.0:0".parse().unwrap(),
            data_dir: dir.path().to_path_buf(),
            expire_days: 30,
        };
        let store = PasteStore::new(&config).unwrap();
        let meta = store.create(b"hello", "text/plain", None, "secret", "temporary").unwrap();
        assert_eq!(meta.content_type, "text/plain");
        assert_eq!(meta.data_policy, "temporary");

        let (_meta, content) = store.open(&meta.id).unwrap();
        assert_eq!(content, b"hello");

        store.delete(&meta.id, "secret").unwrap();
        assert!(store.open(&meta.id).is_err());
    }

    #[test]
    fn test_password_protected_paste() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            listen_addr: "0.0.0.0:0".parse().unwrap(),
            data_dir: dir.path().to_path_buf(),
            expire_days: 30,
        };
        let store = PasteStore::new(&config).unwrap();
        let meta = store.create(b"secret content", "text/plain", Some("pass123"), "tok", "temporary").unwrap();
        assert!(meta.password_hash.is_some());
    }

    #[test]
    fn test_admin_delete() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            listen_addr: "0.0.0.0:0".parse().unwrap(),
            data_dir: dir.path().to_path_buf(),
            expire_days: 30,
        };
        let store = PasteStore::new(&config).unwrap();
        let meta = store.create(b"data", "text/plain", None, "tok", "temporary").unwrap();
        store.admin_delete(&meta.id).unwrap();
        assert!(store.open(&meta.id).is_err());
    }
}

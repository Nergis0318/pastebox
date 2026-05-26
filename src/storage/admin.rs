use crate::config::Config;
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rand::Rng;
use rand::rngs::OsRng;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct AdminStore {
    pool: Pool<SqliteConnectionManager>,
}

#[derive(Debug, Clone)]
pub struct AdminPasteItem {
    pub id: String,
    pub created_at: String,
    pub expires_at: String,
    pub data_policy: String,
    pub size: u64,
    pub content_type: String,
    pub protected: bool,
}

impl AdminStore {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let db_path = config.data_dir.join("pastebox.db");
        let manager = SqliteConnectionManager::file(&db_path);
        let pool = Pool::builder()
            .max_size(4)
            .build(manager)?;

        let conn = pool.get()?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pastebox_admin (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                username TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                created_at_unix INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS admin_sessions (
                token_hash TEXT PRIMARY KEY,
                created_at_unix INTEGER NOT NULL,
                expires_at_unix INTEGER NOT NULL
            );"
        )?;

        Ok(Self { pool })
    }

    pub fn admin_exists(&self) -> anyhow::Result<bool> {
        let conn = self.pool.get()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pastebox_admin",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn create_admin(&self, username: &str, password: &str) -> anyhow::Result<()> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("argon2 error: {e}"))?
            .to_string();

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT OR REPLACE INTO pastebox_admin (id, username, password_hash, created_at_unix)
             VALUES (1, ?1, ?2, ?3)",
            rusqlite::params![username, hash, now],
        )?;
        Ok(())
    }

    pub fn authenticate_admin(&self, username: &str, password: &str) -> anyhow::Result<bool> {
        let conn = self.pool.get()?;
        let result = conn.query_row(
            "SELECT password_hash FROM pastebox_admin WHERE username = ?1",
            rusqlite::params![username],
            |row| row.get::<_, String>(0),
        );

        let stored_hash = match result {
            Ok(h) => h,
            Err(_) => return Ok(false),
        };

        let parsed = PasswordHash::new(&stored_hash)
            .map_err(|e| anyhow::anyhow!("invalid password hash: {e}"))?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|e| anyhow::anyhow!("argon2 verify error: {e}"))
            .map(|_| true)
            .or(Ok(false))
    }

    pub fn create_session(&self) -> anyhow::Result<String> {
        let token = random_token(48);
        let token_hash = hex::encode(Sha256::digest(token.as_bytes()));

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let expires = now + 86400;

        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO admin_sessions (token_hash, created_at_unix, expires_at_unix)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![token_hash, now, expires],
        )?;

        Ok(token)
    }

    pub fn validate_session(&self, token: &str) -> anyhow::Result<bool> {
        let token_hash = hex::encode(Sha256::digest(token.as_bytes()));
        let conn = self.pool.get()?;
        let result = conn.query_row(
            "SELECT expires_at_unix FROM admin_sessions WHERE token_hash = ?1",
            rusqlite::params![token_hash],
            |row| row.get::<_, i64>(0),
        );

        let expires: i64 = match result {
            Ok(e) => e,
            Err(_) => return Ok(false),
        };

        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        Ok(now < expires)
    }

    pub fn delete_session(&self, token: &str) -> anyhow::Result<()> {
        let token_hash = hex::encode(Sha256::digest(token.as_bytes()));
        let conn = self.pool.get()?;
        conn.execute(
            "DELETE FROM admin_sessions WHERE token_hash = ?1",
            rusqlite::params![token_hash],
        )?;
        Ok(())
    }
}

fn random_token(len: usize) -> String {
    let alphabet: Vec<u8> = (b'0'..=b'9')
        .chain(b'A'..=b'Z')
        .chain(b'a'..=b'z')
        .collect();
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..alphabet.len());
            alphabet[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use tempfile::TempDir;

    fn test_config() -> (Config, TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            listen_addr: "0.0.0.0:0".parse().unwrap(),
            data_dir: dir.path().to_path_buf(),
            expire_days: 30,
        };
        (config, dir)
    }

    #[test]
    fn test_admin_lifecycle() {
        let (config, _dir) = test_config();
        let store = AdminStore::new(&config).unwrap();
        assert!(!store.admin_exists().unwrap());

        store.create_admin("admin", "password123").unwrap();
        assert!(store.admin_exists().unwrap());

        assert!(store.authenticate_admin("admin", "password123").unwrap());
        assert!(!store.authenticate_admin("admin", "wrong").unwrap());
    }

    #[test]
    fn test_session_lifecycle() {
        let (config, _dir) = test_config();
        let store = AdminStore::new(&config).unwrap();
        store.create_admin("admin", "pass").unwrap();

        let token = store.create_session().unwrap();
        assert!(store.validate_session(&token).unwrap());

        store.delete_session(&token).unwrap();
        assert!(!store.validate_session(&token).unwrap());
    }
}

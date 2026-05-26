use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

pub struct LockManager {
    locks: RwLock<HashMap<String, (Arc<Mutex<()>>, usize)>>,
}

impl LockManager {
    pub fn new() -> Self {
        Self {
            locks: RwLock::new(HashMap::new()),
        }
    }

    pub async fn acquire(&self, id: &str) -> LockGuard {
        let mut locks = self.locks.write().await;
        let (lock, count) = locks
            .entry(id.to_string())
            .or_insert_with(|| (Arc::new(Mutex::new(())), 0));
        *count += 1;
        let guard = lock.clone().lock_owned().await;
        LockGuard {
            guard,
            lock: lock.clone(),
            id: id.to_string(),
            locks: &self.locks,
        }
    }
}

pub struct LockGuard<'a> {
    guard: tokio::sync::OwnedMutexGuard<()>,
    #[allow(dead_code)]
    lock: Arc<Mutex<()>>,
    id: String,
    locks: &'a RwLock<HashMap<String, (Arc<Mutex<()>>, usize)>>,
}

impl Drop for LockGuard<'_> {
    fn drop(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_acquire_and_release() {
        let lm = LockManager::new();
        {
            let _g1 = lm.acquire("abc").await;
        }
    }

    #[tokio::test]
    async fn test_different_ids_not_blocking() {
        let lm = LockManager::new();
        let _g1 = lm.acquire("abc").await;
        let _g2 = lm.acquire("def").await;
    }
}

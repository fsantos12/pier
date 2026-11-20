//! Per-name locks that serialise "check, then create" for networks and volumes.
//!
//! Docker has no atomic create-unless-exists for these kinds (volumes are silently
//! reused, network names may repeat), so [`DockerClient`](super::DockerClient) checks
//! for the name first. Without a lock, parallel requests for one name would all pass the
//! check and all "succeed". The lock is per `(kind, name)`, so unrelated creates still
//! run in parallel.
//!
//! This only protects creates that go through this client. Another Docker user creating
//! the same name in the same instant is still a race that Pier cannot close.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

/// The resource kinds that need a create lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Kind {
    Network,
    Volume,
}

type Key = (Kind, String);

/// Registry of the locks currently held or awaited.
///
/// An entry exists only while somebody holds or waits for it, so the map does not grow
/// with the number of names ever created.
#[derive(Debug, Default)]
pub(super) struct CreateLocks {
    locks: Mutex<HashMap<Key, Arc<AsyncMutex<()>>>>,
}

/// Held for the duration of a check-and-create; releases and tidies up on drop, also
/// when the request future is cancelled.
#[derive(Debug)]
pub(super) struct CreateGuard<'a> {
    registry: &'a CreateLocks,
    key: Key,
    lock: Arc<AsyncMutex<()>>,
    permit: Option<OwnedMutexGuard<()>>,
}

impl CreateLocks {
    /// Waits until no other create of `(kind, name)` is in flight and returns the guard.
    pub(super) async fn acquire(&self, kind: Kind, name: &str) -> CreateGuard<'_> {
        let key = (kind, name.to_string());
        let lock = self
            .map()
            .entry(key.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let permit = lock.clone().lock_owned().await;
        CreateGuard {
            registry: self,
            key,
            lock,
            permit: Some(permit),
        }
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<Key, Arc<AsyncMutex<()>>>> {
        self.locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map().len()
    }
}

impl Drop for CreateGuard<'_> {
    fn drop(&mut self) {
        // Release first, so a waiter can proceed while we tidy up.
        drop(self.permit.take());
        let mut map = self.registry.map();
        // Two references left means only the map and this guard: nobody waits. Callers
        // clone the Arc while holding the map lock, so nobody can join after this check.
        if Arc::strong_count(&self.lock) == 2 {
            map.remove(&self.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn same_name_is_serialised_and_the_map_empties() {
        let locks = Arc::new(CreateLocks::default());
        let inside = Arc::new(AtomicUsize::new(0));
        let max_inside = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let (locks, inside, max_inside) = (locks.clone(), inside.clone(), max_inside.clone());
            tasks.push(tokio::spawn(async move {
                let _guard = locks.acquire(Kind::Volume, "data").await;
                let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                max_inside.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(2)).await;
                inside.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(max_inside.load(Ordering::SeqCst), 1, "creates overlapped");
        assert_eq!(locks.len(), 0, "lock entries leaked");
    }

    #[tokio::test]
    async fn different_names_and_kinds_do_not_block_each_other() {
        let locks = CreateLocks::default();
        let a = locks.acquire(Kind::Volume, "x").await;
        let b = locks.acquire(Kind::Volume, "y").await;
        let c = locks.acquire(Kind::Network, "x").await;
        assert_eq!(locks.len(), 3);
        drop((a, b, c));
        assert_eq!(locks.len(), 0);
    }

    #[tokio::test]
    async fn a_cancelled_waiter_leaves_nothing_behind() {
        let locks = CreateLocks::default();
        let held = locks.acquire(Kind::Network, "n").await;
        let waiting =
            tokio::time::timeout(Duration::from_millis(10), locks.acquire(Kind::Network, "n"))
                .await;
        assert!(waiting.is_err(), "second acquire should have been blocked");
        drop(held);
        assert_eq!(locks.len(), 0);
    }
}

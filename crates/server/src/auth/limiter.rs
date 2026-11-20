//! In-memory brute-force protection for the login endpoint.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Failures allowed before a lockout starts.
pub const FREE_ATTEMPTS: u32 = 5;
/// First lockout length; it doubles with each further failure.
pub const BASE_LOCKOUT: Duration = Duration::from_secs(15);
/// Longest lockout.
pub const MAX_LOCKOUT: Duration = Duration::from_secs(15 * 60);
/// Entries idle for this long are forgotten.
const FORGET_AFTER: Duration = Duration::from_secs(60 * 60);
/// Upper bound on tracked keys, so spraying random usernames cannot exhaust memory.
const MAX_ENTRIES: usize = 10_000;

/// Longest username (in bytes) that gets its own entry; longer ones share one bucket.
///
/// Matches the server's username limit, so a longer name can never be a real account
/// and the map's memory stays bounded no matter how large the request body is.
const MAX_KEY_LEN: usize = 64;
/// Shared key for every username longer than [`MAX_KEY_LEN`].
const OVERSIZED_KEY: &str = "\0oversized";

#[derive(Debug, Clone, Copy)]
struct Entry {
    failures: u32,
    locked_until: Option<Instant>,
    last_failure: Instant,
}

impl Entry {
    fn new(now: Instant) -> Self {
        Self {
            failures: 0,
            locked_until: None,
            last_failure: now,
        }
    }

    fn is_locked(&self, now: Instant) -> bool {
        self.locked_until.is_some_and(|until| until > now)
    }

    /// Counts one failure and starts or extends the lockout when due.
    fn fail(&mut self, now: Instant) {
        if now.duration_since(self.last_failure) >= FORGET_AFTER {
            self.failures = 0;
        }
        self.failures = self.failures.saturating_add(1);
        self.last_failure = now;
        if self.failures >= FREE_ATTEMPTS {
            let doublings = (self.failures - FREE_ATTEMPTS).min(16);
            let lockout = BASE_LOCKOUT
                .saturating_mul(1u32 << doublings)
                .min(MAX_LOCKOUT);
            self.locked_until = Some(now + lockout);
        }
    }
}

#[derive(Debug, Default)]
struct State {
    entries: HashMap<String, Entry>,
    /// Shared backoff for keys that could not get their own entry because the map is
    /// full of locked-out ones. It keeps unrecorded usernames rate-limited, so filling
    /// the map never turns into a way to guess passwords without limit.
    overflow: Option<Entry>,
}

/// Tracks failed logins per (lower-cased) username and locks out repeat offenders.
///
/// Unknown usernames are tracked exactly like real ones, so lockouts do not reveal
/// which accounts exist.
///
/// Memory is bounded by [`MAX_ENTRIES`] and entries are only evicted when that is safe:
///
/// 1. Locked-out entries are never evicted.
/// 2. Idle entries (no failure for [`FORGET_AFTER`]) are dropped first.
/// 3. Otherwise the unlocked entry with the fewest failures is dropped, so a flood of
///    one-off junk names cannot push out an account that is being attacked.
/// 4. If every entry is locked, the new key goes into a single shared overflow bucket
///    that backs off like any other key. While that bucket is locked, usernames without
///    their own entry are refused, which is the price of the flood.
#[derive(Debug, Default)]
pub struct LoginLimiter {
    state: Mutex<State>,
}

fn normalize(key: &str) -> String {
    if key.len() > MAX_KEY_LEN {
        OVERSIZED_KEY.to_string()
    } else {
        key.to_lowercase()
    }
}

impl LoginLimiter {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Returns `Err(seconds)` when `key` is currently locked out.
    ///
    /// # Errors
    /// The error value is the number of seconds (at least 1) until the next attempt.
    pub fn check(&self, key: &str, now: Instant) -> Result<(), u64> {
        let state = self.lock();
        let entry = match state.entries.get(&normalize(key)) {
            Some(entry) => Some(entry),
            // No entry of its own: only the overflow bucket can apply, and only while
            // the map has no room left.
            None if state.entries.len() >= MAX_ENTRIES => state.overflow.as_ref(),
            None => None,
        };
        match entry.and_then(|e| e.locked_until) {
            Some(until) if until > now => Err((until - now).as_secs().max(1)),
            _ => Ok(()),
        }
    }

    /// Records a failed attempt and starts or extends the lockout when due.
    pub fn record_failure(&self, key: &str, now: Instant) {
        let key = normalize(key);
        let mut state = self.lock();
        if !state.entries.contains_key(&key)
            && state.entries.len() >= MAX_ENTRIES
            && !make_room(&mut state.entries, now)
        {
            state
                .overflow
                .get_or_insert_with(|| Entry::new(now))
                .fail(now);
            return;
        }
        state
            .entries
            .entry(key)
            .or_insert_with(|| Entry::new(now))
            .fail(now);
    }

    /// Forgets a key after a successful login.
    pub fn record_success(&self, key: &str) {
        self.lock().entries.remove(&normalize(key));
    }
}

/// Frees one slot without touching locked-out entries; returns `false` if impossible.
fn make_room(entries: &mut HashMap<String, Entry>, now: Instant) -> bool {
    entries.retain(|_, e| e.is_locked(now) || now.duration_since(e.last_failure) < FORGET_AFTER);
    if entries.len() < MAX_ENTRIES {
        return true;
    }
    let victim = entries
        .iter()
        .filter(|(_, e)| !e.is_locked(now))
        .min_by_key(|(_, e)| (e.failures, e.last_failure))
        .map(|(k, _)| k.clone());
    victim.is_some_and(|k| entries.remove(&k).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_after_repeated_failures_and_recovers() {
        let limiter = LoginLimiter::default();
        let t0 = Instant::now();
        for _ in 0..FREE_ATTEMPTS - 1 {
            limiter.record_failure("Admin", t0);
            assert!(limiter.check("admin", t0).is_ok());
        }
        limiter.record_failure("admin", t0);
        let wait = limiter.check("ADMIN", t0).unwrap_err();
        assert!((1..=BASE_LOCKOUT.as_secs()).contains(&wait));
        assert!(limiter.check("admin", t0 + BASE_LOCKOUT).is_ok());
    }

    #[test]
    fn lockout_grows_and_is_capped() {
        let limiter = LoginLimiter::default();
        let t0 = Instant::now();
        for _ in 0..FREE_ATTEMPTS {
            limiter.record_failure("u", t0);
        }
        let first = limiter.check("u", t0).unwrap_err();
        limiter.record_failure("u", t0);
        let second = limiter.check("u", t0).unwrap_err();
        assert!(second > first);
        for _ in 0..40 {
            limiter.record_failure("u", t0);
        }
        assert!(limiter.check("u", t0).unwrap_err() <= MAX_LOCKOUT.as_secs());
    }

    #[test]
    fn success_clears_and_keys_are_independent() {
        let limiter = LoginLimiter::default();
        let t0 = Instant::now();
        for _ in 0..FREE_ATTEMPTS {
            limiter.record_failure("a", t0);
        }
        assert!(limiter.check("a", t0).is_err());
        assert!(limiter.check("b", t0).is_ok());
        limiter.record_success("a");
        assert!(limiter.check("a", t0).is_ok());
    }

    #[test]
    fn memory_is_bounded() {
        let limiter = LoginLimiter::default();
        let t0 = Instant::now();
        for i in 0..MAX_ENTRIES + 50 {
            limiter.record_failure(&format!("user{i}"), t0);
        }
        assert!(limiter.lock().entries.len() <= MAX_ENTRIES);
    }

    fn lock_out(limiter: &LoginLimiter, key: &str, now: Instant) {
        for _ in 0..FREE_ATTEMPTS {
            limiter.record_failure(key, now);
        }
    }

    #[test]
    fn locked_entries_survive_a_flood_of_locked_junk() {
        let limiter = LoginLimiter::default();
        let t0 = Instant::now();
        lock_out(&limiter, "admin", t0);
        for i in 0..MAX_ENTRIES + 500 {
            lock_out(&limiter, &format!("junk{i}"), t0);
        }
        assert!(limiter.check("admin", t0).is_err());
        assert!(limiter.lock().entries.len() <= MAX_ENTRIES);
    }

    #[test]
    fn untracked_admin_stays_limited_when_the_map_is_full_of_locked_keys() {
        let limiter = LoginLimiter::default();
        let t0 = Instant::now();
        for i in 0..MAX_ENTRIES {
            lock_out(&limiter, &format!("junk{i}"), t0);
        }
        assert_eq!(limiter.lock().entries.len(), MAX_ENTRIES);
        // The admin has no entry and no room to get one: failures go to the overflow
        // bucket, which must lock after the usual number of attempts.
        for _ in 0..FREE_ATTEMPTS - 1 {
            limiter.record_failure("admin", t0);
            assert!(limiter.check("admin", t0).is_ok());
        }
        limiter.record_failure("admin", t0);
        assert!(
            limiter.check("admin", t0).is_err(),
            "admin guessed without limit"
        );
        assert!(
            limiter.check("someone-else", t0).is_err(),
            "bucket is shared"
        );
        assert!(
            limiter.lock().entries.len() <= MAX_ENTRIES,
            "map grew past its cap"
        );
        // The backoff ends, and room appears once the junk locks expire.
        let later = t0 + MAX_LOCKOUT + FORGET_AFTER;
        assert!(limiter.check("admin", later).is_ok());
        limiter.record_failure("admin", later);
        assert!(limiter.lock().entries.contains_key("admin"));
    }

    #[test]
    fn junk_flood_does_not_evict_an_account_under_attack() {
        let limiter = LoginLimiter::default();
        let t0 = Instant::now();
        for _ in 0..FREE_ATTEMPTS - 1 {
            limiter.record_failure("admin", t0);
        }
        for i in 0..MAX_ENTRIES + 500 {
            limiter.record_failure(&format!("junk{i}"), t0);
        }
        // One more failure must trip the lockout, so the counter was not reset.
        limiter.record_failure("admin", t0);
        assert!(limiter.check("admin", t0).is_err());
    }

    #[test]
    fn oversized_usernames_share_one_bounded_entry() {
        let limiter = LoginLimiter::default();
        let t0 = Instant::now();
        for i in 0..100 {
            limiter.record_failure(&format!("{i}{}", "x".repeat(MAX_KEY_LEN + 1)), t0);
        }
        assert_eq!(limiter.lock().entries.len(), 1);
        assert!(limiter.check(&"y".repeat(5000), t0).is_err());
        // A 64-byte username is still a normal, separate key.
        limiter.record_failure(&"a".repeat(MAX_KEY_LEN), t0);
        assert_eq!(limiter.lock().entries.len(), 2);
    }
}

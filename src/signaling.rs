use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MAX_KEY_BYTES: usize = 256;

/// A client-chosen key, e.g. `sdp-req` or `aicast-webrtc/<room>/offer`. Keys
/// are free-form but always live inside the caller's own namespace: the store
/// is keyed by (owner id, key), so no caller can name another owner's data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Key(String);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidKey {
    #[error("key is empty")]
    Empty,
    #[error("key is longer than {MAX_KEY_BYTES} bytes")]
    TooLong,
    #[error("key contains whitespace or control characters")]
    ForbiddenCharacter,
}

impl TryFrom<String> for Key {
    type Error = InvalidKey;

    fn try_from(key: String) -> Result<Self, Self::Error> {
        if key.is_empty() {
            return Err(InvalidKey::Empty);
        }
        if key.len() > MAX_KEY_BYTES {
            return Err(InvalidKey::TooLong);
        }
        if key.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(InvalidKey::ForbiddenCharacter);
        }
        Ok(Self(key))
    }
}

impl From<Key> for String {
    fn from(key: Key) -> Self {
        key.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_value_bytes: usize,
    pub max_entries: usize,
    /// Keeps one owner from filling the shared store with free-form keys.
    pub max_entries_per_owner: usize,
    pub ttl: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // A vanilla-ICE SDP with every codec is well under 16 KiB.
            max_value_bytes: 32 * 1024,
            max_entries: 1024,
            // A signaling session uses about five keys per room.
            max_entries_per_owner: 64,
            ttl: Duration::from_secs(15 * 60),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SetError {
    ValueTooLarge,
    StoreFull,
    TooManyKeys,
}

#[derive(Debug)]
struct Entry {
    value: String,
    expires_at: Instant,
}

#[derive(Debug)]
pub struct SignalingStore {
    limits: Limits,
    entries: Mutex<HashMap<(i64, Key), Entry>>,
}

impl SignalingStore {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn set(&self, owner: i64, key: Key, value: String) -> Result<(), SetError> {
        self.set_at(owner, key, value, Instant::now())
    }

    pub fn get(&self, owner: i64, key: Key) -> Option<String> {
        self.get_at(owner, key, Instant::now())
    }

    pub fn delete(&self, owner: i64, key: Key) {
        self.lock().remove(&(owner, key));
    }

    fn set_at(&self, owner: i64, key: Key, value: String, now: Instant) -> Result<(), SetError> {
        if value.len() > self.limits.max_value_bytes {
            return Err(SetError::ValueTooLarge);
        }
        let mut entries = self.lock();
        let key = (owner, key);
        if !entries.contains_key(&key) {
            let owned = |entries: &HashMap<(i64, Key), Entry>| {
                entries.keys().filter(|(o, _)| *o == owner).count()
            };
            if entries.len() >= self.limits.max_entries
                || owned(&entries) >= self.limits.max_entries_per_owner
            {
                entries.retain(|_, entry| entry.expires_at > now);
            }
            if owned(&entries) >= self.limits.max_entries_per_owner {
                return Err(SetError::TooManyKeys);
            }
            if entries.len() >= self.limits.max_entries {
                return Err(SetError::StoreFull);
            }
        }
        entries.insert(
            key,
            Entry {
                value,
                expires_at: now + self.limits.ttl,
            },
        );
        Ok(())
    }

    fn get_at(&self, owner: i64, key: Key, now: Instant) -> Option<String> {
        let mut entries = self.lock();
        let key = (owner, key);
        match entries.get(&key) {
            Some(entry) if entry.expires_at > now => Some(entry.value.clone()),
            Some(_) => {
                entries.remove(&key);
                None
            }
            None => None,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(i64, Key), Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str) -> Key {
        Key::try_from(k.to_owned()).unwrap()
    }

    fn store(max_entries: usize, max_entries_per_owner: usize) -> SignalingStore {
        SignalingStore::new(Limits {
            max_value_bytes: 8,
            max_entries,
            max_entries_per_owner,
            ttl: Duration::from_secs(60),
        })
    }

    #[test]
    fn key_validation() {
        for ok in ["sdp-req", "aicast-webrtc/r4nd0m/offer", "ルーム/offer"] {
            assert!(Key::try_from(ok.to_owned()).is_ok(), "{ok}");
        }
        assert!(Key::try_from("k".repeat(MAX_KEY_BYTES)).is_ok());
        assert_eq!(Key::try_from(String::new()), Err(InvalidKey::Empty));
        assert_eq!(
            Key::try_from("k".repeat(MAX_KEY_BYTES + 1)),
            Err(InvalidKey::TooLong)
        );
        for bad in ["a b", "a\tb", "a\nb", "a\u{0}b", "a\u{7f}b", "a\u{3000}b"] {
            assert_eq!(
                Key::try_from(bad.to_owned()),
                Err(InvalidKey::ForbiddenCharacter),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn owners_do_not_share_keys() {
        let store = store(10, 10);
        store.set(1, key("room/offer"), "offer".into()).unwrap();
        assert_eq!(store.get(1, key("room/offer")).as_deref(), Some("offer"));
        assert_eq!(store.get(2, key("room/offer")), None);
        store.delete(2, key("room/offer"));
        assert_eq!(store.get(1, key("room/offer")).as_deref(), Some("offer"));
    }

    #[test]
    fn rejects_oversized_values() {
        let store = store(10, 10);
        assert_eq!(
            store.set(1, key("k"), "123456789".into()),
            Err(SetError::ValueTooLarge)
        );
        assert!(store.set(1, key("k"), "12345678".into()).is_ok());
    }

    #[test]
    fn entry_count_is_bounded_and_expired_entries_free_space() {
        let store = store(2, 2);
        let t0 = Instant::now();
        store.set_at(1, key("k"), "a".into(), t0).unwrap();
        store.set_at(2, key("k"), "b".into(), t0).unwrap();
        assert_eq!(
            store.set_at(3, key("k"), "c".into(), t0),
            Err(SetError::StoreFull)
        );
        assert!(
            store.set_at(1, key("k"), "a2".into(), t0).is_ok(),
            "overwrite needs no room"
        );

        let later = t0 + Duration::from_secs(61);
        assert_eq!(store.get_at(1, key("k"), later), None, "entries expire");
        assert!(store.set_at(3, key("k"), "c".into(), later).is_ok());
    }

    #[test]
    fn one_owner_cannot_fill_the_store() {
        let store = store(10, 2);
        let t0 = Instant::now();
        store.set_at(1, key("a"), "v".into(), t0).unwrap();
        store.set_at(1, key("b"), "v".into(), t0).unwrap();
        assert_eq!(
            store.set_at(1, key("c"), "v".into(), t0),
            Err(SetError::TooManyKeys)
        );
        assert!(store.set_at(1, key("a"), "v2".into(), t0).is_ok());
        assert!(store.set_at(2, key("c"), "v".into(), t0).is_ok());
        let later = t0 + Duration::from_secs(61);
        assert!(store.set_at(1, key("c"), "v".into(), later).is_ok());
    }
}

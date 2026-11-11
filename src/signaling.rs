use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The only keys a client can address. Every slot belongs to the logged-in
/// user, so a client cannot name another user's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Slot {
    SdpReq,
    SdpRes,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_value_bytes: usize,
    pub max_entries: usize,
    pub ttl: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // A vanilla-ICE SDP with every codec is well under 16 KiB.
            max_value_bytes: 32 * 1024,
            max_entries: 1024,
            ttl: Duration::from_secs(15 * 60),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SetError {
    ValueTooLarge,
    StoreFull,
}

#[derive(Debug)]
struct Entry {
    value: String,
    expires_at: Instant,
}

#[derive(Debug)]
pub struct SignalingStore {
    limits: Limits,
    entries: Mutex<HashMap<(i64, Slot), Entry>>,
}

impl SignalingStore {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn set(&self, owner: i64, slot: Slot, value: String) -> Result<(), SetError> {
        self.set_at(owner, slot, value, Instant::now())
    }

    pub fn get(&self, owner: i64, slot: Slot) -> Option<String> {
        self.get_at(owner, slot, Instant::now())
    }

    pub fn delete(&self, owner: i64, slot: Slot) {
        self.lock().remove(&(owner, slot));
    }

    fn set_at(&self, owner: i64, slot: Slot, value: String, now: Instant) -> Result<(), SetError> {
        if value.len() > self.limits.max_value_bytes {
            return Err(SetError::ValueTooLarge);
        }
        let mut entries = self.lock();
        let key = (owner, slot);
        if !entries.contains_key(&key) && entries.len() >= self.limits.max_entries {
            entries.retain(|_, entry| entry.expires_at > now);
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

    fn get_at(&self, owner: i64, slot: Slot, now: Instant) -> Option<String> {
        let mut entries = self.lock();
        let key = (owner, slot);
        match entries.get(&key) {
            Some(entry) if entry.expires_at > now => Some(entry.value.clone()),
            Some(_) => {
                entries.remove(&key);
                None
            }
            None => None,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(i64, Slot), Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(max_entries: usize) -> SignalingStore {
        SignalingStore::new(Limits {
            max_value_bytes: 8,
            max_entries,
            ttl: Duration::from_secs(60),
        })
    }

    #[test]
    fn owners_do_not_share_slots() {
        let store = store(10);
        store.set(1, Slot::SdpReq, "offer".into()).unwrap();
        assert_eq!(store.get(1, Slot::SdpReq).as_deref(), Some("offer"));
        assert_eq!(store.get(2, Slot::SdpReq), None);
        store.delete(2, Slot::SdpReq);
        assert_eq!(store.get(1, Slot::SdpReq).as_deref(), Some("offer"));
    }

    #[test]
    fn rejects_oversized_values() {
        let store = store(10);
        assert_eq!(
            store.set(1, Slot::SdpReq, "123456789".into()),
            Err(SetError::ValueTooLarge)
        );
        assert!(store.set(1, Slot::SdpReq, "12345678".into()).is_ok());
    }

    #[test]
    fn entry_count_is_bounded_and_expired_entries_free_space() {
        let store = store(2);
        let t0 = Instant::now();
        store.set_at(1, Slot::SdpReq, "a".into(), t0).unwrap();
        store.set_at(2, Slot::SdpReq, "b".into(), t0).unwrap();
        assert_eq!(
            store.set_at(3, Slot::SdpReq, "c".into(), t0),
            Err(SetError::StoreFull)
        );
        assert!(
            store.set_at(1, Slot::SdpReq, "a2".into(), t0).is_ok(),
            "overwrite needs no room"
        );

        let later = t0 + Duration::from_secs(61);
        assert_eq!(store.get_at(1, Slot::SdpReq, later), None, "entries expire");
        assert!(store.set_at(3, Slot::SdpReq, "c".into(), later).is_ok());
    }
}

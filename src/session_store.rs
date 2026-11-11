use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use time::OffsetDateTime;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, SessionStore};

/// In-memory session store with a hard cap on the number of sessions.
/// `tower_sessions::MemoryStore` never drops expired records, so memory would
/// only ever grow.
#[derive(Debug, Clone)]
pub struct BoundedMemoryStore {
    max_sessions: usize,
    records: Arc<Mutex<HashMap<Id, Record>>>,
}

impl BoundedMemoryStore {
    pub fn new(max_sessions: usize) -> Self {
        Self {
            max_sessions,
            records: Arc::default(),
        }
    }

    pub fn spawn_purger(&self, period: Duration) -> tokio::task::JoinHandle<()> {
        let store = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            loop {
                interval.tick().await;
                store.lock().retain(|_, record| is_active(record));
            }
        })
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Id, Record>> {
        self.records.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn make_room(&self, records: &mut HashMap<Id, Record>) -> session_store::Result<()> {
        if records.len() >= self.max_sessions {
            records.retain(|_, record| is_active(record));
        }
        if records.len() >= self.max_sessions {
            return Err(session_store::Error::Backend(
                "session store is full".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl SessionStore for BoundedMemoryStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let mut records = self.lock();
        while records.contains_key(&record.id) {
            record.id = Id::default();
        }
        self.make_room(&mut records)?;
        records.insert(record.id, record.clone());
        Ok(())
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let mut records = self.lock();
        if !records.contains_key(&record.id) {
            self.make_room(&mut records)?;
        }
        records.insert(record.id, record.clone());
        Ok(())
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        Ok(self.lock().get(id).filter(|r| is_active(r)).cloned())
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        self.lock().remove(id);
        Ok(())
    }
}

fn is_active(record: &Record) -> bool {
    record.expiry_date > OffsetDateTime::now_utc()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(expires_in: time::Duration) -> Record {
        Record {
            id: Id::default(),
            data: Default::default(),
            expiry_date: OffsetDateTime::now_utc() + expires_in,
        }
    }

    #[tokio::test]
    async fn capacity_is_enforced_and_expired_records_are_reclaimed() {
        let store = BoundedMemoryStore::new(2);
        store
            .create(&mut record(time::Duration::hours(1)))
            .await
            .unwrap();
        store
            .create(&mut record(-time::Duration::seconds(1)))
            .await
            .unwrap();
        // The expired record makes room for this one.
        store
            .create(&mut record(time::Duration::hours(1)))
            .await
            .unwrap();
        assert_eq!(store.len(), 2);
        assert!(store
            .create(&mut record(time::Duration::hours(1)))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn expired_records_are_not_loaded() {
        let store = BoundedMemoryStore::new(2);
        let mut expired = record(-time::Duration::seconds(1));
        store.create(&mut expired).await.unwrap();
        assert!(store.load(&expired.id).await.unwrap().is_none());
    }
}

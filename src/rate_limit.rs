use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone, Copy)]
pub struct Quota {
    pub burst: u32,
    pub per_second: f64,
}

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    updated: Instant,
}

/// Token bucket per key. The number of tracked keys is capped so the limiter
/// itself cannot be used to exhaust memory.
#[derive(Debug)]
pub struct RateLimiter<K> {
    quota: Quota,
    max_keys: usize,
    buckets: Mutex<HashMap<K, Bucket>>,
}

impl<K: Eq + Hash> RateLimiter<K> {
    pub fn new(quota: Quota, max_keys: usize) -> Self {
        Self {
            quota,
            max_keys,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn check(&self, key: K) -> bool {
        self.check_at(key, Instant::now())
    }

    fn check_at(&self, key: K, now: Instant) -> bool {
        let burst = f64::from(self.quota.burst);
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if !buckets.contains_key(&key) && buckets.len() >= self.max_keys {
            // A bucket that has refilled completely is indistinguishable from a new one.
            buckets.retain(|_, bucket| self.refilled(bucket, now) < burst);
            if buckets.len() >= self.max_keys {
                return false;
            }
        }
        let bucket = buckets.entry(key).or_insert(Bucket {
            tokens: burst,
            updated: now,
        });
        bucket.tokens = self.refilled(bucket, now);
        bucket.updated = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn refilled(&self, bucket: &Bucket, now: Instant) -> f64 {
        let elapsed = now.saturating_duration_since(bucket.updated).as_secs_f64();
        (bucket.tokens + elapsed * self.quota.per_second).min(f64::from(self.quota.burst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const QUOTA: Quota = Quota {
        burst: 3,
        per_second: 1.0,
    };

    #[test]
    fn allows_burst_then_refills() {
        let limiter = RateLimiter::new(QUOTA, 10);
        let t0 = Instant::now();
        assert!((0..3).all(|_| limiter.check_at("a", t0)));
        assert!(!limiter.check_at("a", t0));
        assert!(limiter.check_at("b", t0), "keys are independent");
        assert!(limiter.check_at("a", t0 + Duration::from_secs(1)));
        assert!(!limiter.check_at("a", t0 + Duration::from_secs(1)));
    }

    #[test]
    fn key_capacity_is_bounded() {
        let limiter = RateLimiter::new(QUOTA, 2);
        let t0 = Instant::now();
        assert!(limiter.check_at(1, t0));
        assert!(limiter.check_at(2, t0));
        assert!(
            !limiter.check_at(3, t0),
            "no room while other buckets are in use"
        );
        assert!(
            limiter.check_at(3, t0 + Duration::from_secs(10)),
            "fully refilled buckets are evicted"
        );
    }
}

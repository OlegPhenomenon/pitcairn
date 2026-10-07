//! Sliding-window in-memory rate limiter (login: 10/min per ip+email).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    window: Duration,
    max: usize,
    hits: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    pub fn new(window: Duration, max: usize) -> Self {
        RateLimiter { window, max, hits: Mutex::new(HashMap::new()) }
    }

    /// Record a hit; true if the key is within the limit.
    pub fn allow(&self, key: &str) -> bool {
        let mut map = self.hits.lock().expect("rate limiter poisoned");
        let now = Instant::now();
        let hits = map.entry(key.to_string()).or_default();
        while hits.front().is_some_and(|t| now.duration_since(*t) > self.window) {
            hits.pop_front();
        }
        if hits.len() >= self.max {
            return false;
        }
        hits.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denies_after_limit() {
        let rl = RateLimiter::new(Duration::from_secs(60), 3);
        assert!(rl.allow("k"));
        assert!(rl.allow("k"));
        assert!(rl.allow("k"));
        assert!(!rl.allow("k"));
        assert!(rl.allow("other"));
    }
}

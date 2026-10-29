use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

/// Token bucket bandwidth limiter for smooth, jitter-free speed throttling.
/// Supports both global limits and per-download limits.
#[derive(Clone, Debug)]
pub struct SpeedLimiter {
    bytes_per_sec: Arc<AtomicU64>,
    state: Arc<Mutex<LimiterState>>,
}

#[derive(Debug)]
struct LimiterState {
    tokens: f64,
    last_update: Instant,
}

impl SpeedLimiter {
    /// Creates a new limiter with the given bytes per second.
    /// 0 means unlimited.
    pub fn new(bytes_per_sec: u64) -> Self {
        Self {
            bytes_per_sec: Arc::new(AtomicU64::new(bytes_per_sec)),
            state: Arc::new(Mutex::new(LimiterState {
                tokens: bytes_per_sec as f64,
                last_update: Instant::now(),
            })),
        }
    }

    /// Update the rate limit dynamically at runtime (e.g., from GUI speed slider).
    pub fn set_limit(&self, bytes_per_sec: u64) {
        self.bytes_per_sec.store(bytes_per_sec, Ordering::Relaxed);
    }

    pub fn get_limit(&self) -> u64 {
        self.bytes_per_sec.load(Ordering::Relaxed)
    }

    /// Acquires permission to send/receive `amount` bytes.
    /// Asynchronously sleeps if bucket is depleted.
    pub async fn acquire(&self, amount: usize) {
        let limit = self.bytes_per_sec.load(Ordering::Relaxed);
        if limit == 0 || amount == 0 {
            return;
        }

        let max_tokens = limit as f64;
        let mut state = self.state.lock().await;

        loop {
            let now = Instant::now();
            let elapsed = now.duration_since(state.last_update).as_secs_f64();
            state.last_update = now;

            // Replenish tokens based on elapsed time
            state.tokens = (state.tokens + elapsed * max_tokens).min(max_tokens);

            if state.tokens >= amount as f64 {
                state.tokens -= amount as f64;
                return;
            }

            // Calculate needed sleep duration
            let needed = amount as f64 - state.tokens;
            let wait_secs = needed / max_tokens;
            drop(state);

            sleep(Duration::from_secs_f64(wait_secs.min(0.2))).await;
            state = self.state.lock().await;
        }
    }
}

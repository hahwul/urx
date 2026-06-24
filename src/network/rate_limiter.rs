use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Minimum-interval rate limiter that paces a provider's HTTP requests.
///
/// Built from a requests-per-second rate; each [`RateLimiter::acquire`] blocks
/// until at least `1/rate` seconds have elapsed since the previous acquire, so
/// requests go out no faster than the configured rate. Cloning shares the same
/// timestamp, so every request paced by a given limiter is throttled together.
///
/// Providers build one per `fetch_urls` call and `acquire()` before each
/// request, which paces a domain's (often paginated) requests. This is what
/// makes `--rate-limit` / `--rate-limit-by` actually take effect — previously
/// the configured rate was stored but never enforced.
#[derive(Clone, Debug)]
pub struct RateLimiter {
    last: Arc<Mutex<Option<Instant>>>,
    min_interval: Duration,
}

impl RateLimiter {
    /// Build a limiter for `requests_per_sec`. Returns `None` for a
    /// non-positive or non-finite rate, i.e. "no limiting".
    pub fn new(requests_per_sec: f32) -> Option<Self> {
        if requests_per_sec <= 0.0 || !requests_per_sec.is_finite() {
            return None;
        }
        Some(Self {
            last: Arc::new(Mutex::new(None)),
            min_interval: Duration::from_secs_f32(1.0 / requests_per_sec),
        })
    }

    /// Convenience constructor from an `Option<f32>` rate, so callers can write
    /// `RateLimiter::from_rate(self.rate_limit)`.
    pub fn from_rate(requests_per_sec: Option<f32>) -> Option<Self> {
        requests_per_sec.and_then(Self::new)
    }

    /// Block until issuing the next request respects the configured rate. The
    /// lock is held across the sleep so concurrent callers queue rather than
    /// all firing at once.
    pub async fn acquire(&self) {
        let mut guard = self.last.lock().await;
        if let Some(prev) = *guard {
            let elapsed = prev.elapsed();
            if elapsed < self.min_interval {
                tokio::time::sleep(self.min_interval - elapsed).await;
            }
        }
        *guard = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_positive_rate_disables_limiting() {
        assert!(RateLimiter::new(0.0).is_none());
        assert!(RateLimiter::new(-1.0).is_none());
        assert!(RateLimiter::new(f32::NAN).is_none());
        assert!(RateLimiter::new(f32::INFINITY).is_none());
        assert!(RateLimiter::from_rate(None).is_none());
        assert!(RateLimiter::from_rate(Some(5.0)).is_some());
    }

    #[tokio::test]
    async fn test_acquire_spaces_requests() {
        // 20 req/s => 50ms minimum interval.
        let limiter = RateLimiter::new(20.0).unwrap();
        let start = Instant::now();
        limiter.acquire().await; // first: no wait
        limiter.acquire().await; // second: ~50ms
        limiter.acquire().await; // third: ~50ms
                                 // Two enforced gaps (~100ms); allow scheduler slack.
        assert!(
            start.elapsed() >= Duration::from_millis(90),
            "elapsed too short: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn test_first_acquire_does_not_block() {
        let limiter = RateLimiter::new(1.0).unwrap(); // 1s interval
        let start = Instant::now();
        limiter.acquire().await; // first acquire must be immediate
        assert!(start.elapsed() < Duration::from_millis(200));
    }
}

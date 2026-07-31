//! Exponential backoff with jitter for reconnect loops.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How often an interruptible sleep checks the stop flag.
const POLL_STEP: Duration = Duration::from_millis(200);

/// Doubles a delay on every failure, capped at `max`, reset to `min` on
/// success.
///
/// Fast recovery from a short blip (starts at `min`), without hammering a
/// server that is genuinely down (backs off toward `max`).
pub(crate) struct Backoff {
    min: Duration,
    max: Duration,
    current: Duration,
}

impl Backoff {
    pub(crate) fn new(min: Duration, max: Duration) -> Self {
        let min = min.max(Duration::from_millis(1));
        let max = max.max(min);
        Self {
            min,
            max,
            current: min,
        }
    }

    /// Returns the next delay (with jitter applied) and advances the backoff.
    pub(crate) fn next(&mut self) -> Duration {
        let delay = jittered(self.current);
        self.current = (self.current * 2).min(self.max);
        delay
    }

    /// Restores the starting delay after a successful attempt.
    pub(crate) fn reset(&mut self) {
        self.current = self.min;
    }
}

/// Applies +/-25% jitter so many collectors backing off together do not
/// retry in lockstep. Uses `RandomState`'s OS-seeded hasher rather than
/// pulling in a `rand` dependency for one call site.
fn jittered(base: Duration) -> Duration {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(nanos);
    let r = hasher.finish();

    let factor = 0.75 + (r % 1000) as f64 / 1000.0 * 0.5;
    Duration::from_secs_f64((base.as_secs_f64() * factor).max(0.0))
}

/// Sleeps for `duration`, but returns early in `POLL_STEP` increments if
/// `stop` becomes set, so a long backoff does not delay shutdown.
pub(crate) fn sleep_interruptible(duration: Duration, stop: &AtomicBool) {
    let mut remaining = duration;
    while remaining > Duration::ZERO {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let chunk = remaining.min(POLL_STEP);
        thread::sleep(chunk);
        remaining = remaining.saturating_sub(chunk);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_min_and_caps_at_max() {
        let min = Duration::from_millis(100);
        let max = Duration::from_millis(800);
        let mut backoff = Backoff::new(min, max);

        // Jitter is +/-25%, so bound each step against a widened range
        // rather than asserting exact doubling.
        let bounds = |base: Duration| {
            (
                Duration::from_secs_f64(base.as_secs_f64() * 0.75),
                Duration::from_secs_f64(base.as_secs_f64() * 1.25),
            )
        };

        for expected in [100u64, 200, 400, 800, 800, 800] {
            let expected = Duration::from_millis(expected);
            let (lo, hi) = bounds(expected);
            let got = backoff.next();
            assert!(got >= lo && got <= hi, "{got:?} not within {lo:?}..={hi:?}");
        }
    }

    #[test]
    fn reset_returns_to_min() {
        let min = Duration::from_millis(50);
        let mut backoff = Backoff::new(min, Duration::from_secs(10));

        backoff.next();
        backoff.next();
        backoff.reset();

        let got = backoff.next();
        assert!(got >= Duration::from_millis(37) && got <= Duration::from_millis(63));
    }

    #[test]
    fn max_below_min_is_clamped_up_to_min() {
        let backoff = Backoff::new(Duration::from_secs(5), Duration::from_secs(1));
        assert_eq!(backoff.max, Duration::from_secs(5));
    }

    #[test]
    fn sleep_interruptible_returns_early_when_stop_is_set() {
        let stop = AtomicBool::new(true);
        let start = std::time::Instant::now();
        sleep_interruptible(Duration::from_secs(30), &stop);
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}

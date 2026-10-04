//! NIGHT SKY PATCH: opt-in CPU OnPaint copy/publish counters, not GPU upload timings.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
static ENABLED: AtomicBool = AtomicBool::new(false);
static FRAMES: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static NANOS: AtomicU64 = AtomicU64::new(0);
pub fn enable() { ENABLED.store(true, Ordering::Relaxed); }
pub(crate) fn enabled() -> bool { ENABLED.load(Ordering::Relaxed) }
pub(crate) fn record(bytes: u64, nanos: u64) {
    FRAMES.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(bytes, Ordering::Relaxed);
    NANOS.fetch_add(nanos, Ordering::Relaxed);
}
pub fn snapshot() -> [u64; 3] {
    [FRAMES.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed), NANOS.load(Ordering::Relaxed)]
}

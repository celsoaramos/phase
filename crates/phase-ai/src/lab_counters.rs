//! Diagnostic counters for `ai-ladder` (MagicFinder lab): how often each
//! `StrategicConfig` switch actually changed a number. Relaxed atomics, only
//! bumped on the switched-on path, so the default build pays one branch.
use std::sync::atomic::{AtomicU64, Ordering};

pub static LEAF_EVALS: AtomicU64 = AtomicU64::new(0);
pub static STACK_CREDITS: AtomicU64 = AtomicU64::new(0);
pub static GATE_BLOCKS: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn bump(counter: &AtomicU64, on: bool) {
    if on {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn snapshot() -> [u64; 3] {
    [
        LEAF_EVALS.load(Ordering::Relaxed),
        STACK_CREDITS.load(Ordering::Relaxed),
        GATE_BLOCKS.load(Ordering::Relaxed),
    ]
}

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservationStamp {
    pub generation: u64,
    pub sequence: u64,
    pub timestamp_ms: u64,
}

/// Process-local ordering source for one observation producer.
///
/// A fresh producer receives a fresh generation. Sequence values are assigned at acquisition time,
/// before presentation batching/decoding, so consumers never reconstruct cross-stream order later.
pub struct ObservationSequencer {
    generation: u64,
    next_sequence: AtomicU64,
}

impl ObservationSequencer {
    pub fn new() -> Self {
        Self {
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            next_sequence: AtomicU64::new(1),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn stamp(&self) -> ObservationStamp {
        ObservationStamp {
            generation: self.generation,
            sequence: self.next_sequence.fetch_add(1, Ordering::Relaxed),
            timestamp_ms: now_ms(),
        }
    }
}

impl Default for ObservationSequencer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generations_and_sequences_are_monotonic() {
        let first = ObservationSequencer::new();
        let second = ObservationSequencer::new();
        assert!(second.generation() > first.generation());
        assert_eq!(first.stamp().sequence, 1);
        assert_eq!(first.stamp().sequence, 2);
    }
}

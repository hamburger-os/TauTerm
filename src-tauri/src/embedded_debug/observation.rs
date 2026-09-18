use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObservationPublishReport {
    pub delivered: usize,
    pub dropped: usize,
    pub disconnected: usize,
}

/// Typed, bounded fan-out source for one canonical observation stream.
///
/// Producers publish the canonical record once. Consumers such as decoders, recorders or
/// presentation adapters subscribe independently and therefore cannot create a second hardware
/// reader. Slow consumers lose only their own bounded queue entries.
struct ObservationSubscriber<T> {
    sender: mpsc::SyncSender<T>,
    dropped: Arc<AtomicU64>,
}

pub struct ObservationSubscription<T> {
    receiver: mpsc::Receiver<T>,
    dropped: Arc<AtomicU64>,
}

impl<T> ObservationSubscription<T> {
    pub fn try_recv(&self) -> Result<T, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }

    pub fn recv(&self) -> Result<T, mpsc::RecvError> {
        self.receiver.recv()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

pub struct ObservationSource<T: Clone + Send + 'static> {
    capacity: usize,
    subscribers: Mutex<Vec<ObservationSubscriber<T>>>,
}

impl<T: Clone + Send + 'static> ObservationSource<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            subscribers: Mutex::new(Vec::new()),
        }
    }

    pub fn subscribe(&self) -> ObservationSubscription<T> {
        let (tx, rx) = mpsc::sync_channel(self.capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.push(ObservationSubscriber {
                sender: tx,
                dropped: Arc::clone(&dropped),
            });
        }
        ObservationSubscription {
            receiver: rx,
            dropped,
        }
    }

    pub fn publish(&self, value: &T) -> ObservationPublishReport {
        let mut report = ObservationPublishReport::default();
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.retain(|subscriber| match subscriber.sender.try_send(value.clone()) {
                Ok(()) => {
                    report.delivered += 1;
                    true
                }
                Err(mpsc::TrySendError::Full(_)) => {
                    subscriber.dropped.fetch_add(1, Ordering::Relaxed);
                    report.dropped += 1;
                    true
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    report.disconnected += 1;
                    false
                }
            });
        }
        report
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

    #[test]
    fn observation_source_is_bounded_per_subscriber() {
        let source = ObservationSource::new(1);
        let fast = source.subscribe();
        let slow = source.subscribe();

        let first = source.publish(&1u32);
        assert_eq!(first.delivered, 2);
        assert_eq!(fast.recv().unwrap(), 1);

        let second = source.publish(&2u32);
        assert_eq!(second.delivered, 1);
        assert_eq!(second.dropped, 1);
        assert_eq!(fast.recv().unwrap(), 2);
        assert_eq!(slow.recv().unwrap(), 1);
        assert_eq!(fast.dropped(), 0);
        assert_eq!(slow.dropped(), 1);
    }
}

//! Bounded terminal delivery queues and slow-client detachment.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{MAX_TERMINAL_PAYLOAD, TerminalFrame, TerminalFrameKind};

/// Default bytes retained for one terminal client.
pub const DEFAULT_CLIENT_QUEUE_BYTES: usize = 1024 * 1024;

/// Maximum bytes retained for one terminal client.
pub const MAX_CLIENT_QUEUE_BYTES: usize = 4 * 1024 * 1024;

/// Attach request sent over the control channel before binary output starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalAttachRequest {
    /// Stream to subscribe to.
    pub stream_id: Uuid,
    /// Optional queue bound; the host clamps to its configured limits.
    pub queue_bytes: Option<u32>,
}

/// Successful attach response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalAttachResponse {
    /// Stream subscribed to.
    pub stream_id: Uuid,
    /// Actual bounded queue size.
    pub queue_bytes: u32,
    /// Maximum payload bytes in one binary frame.
    pub max_frame_bytes: u32,
    /// First sequence the client should expect after attaching.
    pub next_sequence: u64,
}

/// Hub limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalLimits {
    /// Maximum payload in one frame, capped at 64 KiB.
    pub max_payload_bytes: usize,
    /// Default queue for a subscriber.
    pub default_queue_bytes: usize,
    /// Maximum queue for a subscriber.
    pub max_queue_bytes: usize,
    /// Maximum concurrently registered streams.
    pub max_sessions: usize,
}

impl Default for TerminalLimits {
    fn default() -> Self {
        Self {
            max_payload_bytes: MAX_TERMINAL_PAYLOAD,
            default_queue_bytes: DEFAULT_CLIENT_QUEUE_BYTES,
            max_queue_bytes: MAX_CLIENT_QUEUE_BYTES,
            max_sessions: 64,
        }
    }
}

impl TerminalLimits {
    fn validate(self) -> Result<Self, TerminalHubError> {
        if self.max_payload_bytes == 0 || self.max_payload_bytes > MAX_TERMINAL_PAYLOAD {
            return Err(TerminalHubError::InvalidLimits(
                "max_payload_bytes must be between 1 and 64 KiB".to_string(),
            ));
        }
        if self.default_queue_bytes == 0 || self.default_queue_bytes > self.max_queue_bytes {
            return Err(TerminalHubError::InvalidLimits(
                "default queue must be between 1 byte and max_queue_bytes".to_string(),
            ));
        }
        if self.max_queue_bytes > MAX_CLIENT_QUEUE_BYTES {
            return Err(TerminalHubError::InvalidLimits(
                "max_queue_bytes exceeds the 4 MiB limit".to_string(),
            ));
        }
        if self.max_sessions == 0 {
            return Err(TerminalHubError::InvalidLimits(
                "max_sessions must be positive".to_string(),
            ));
        }
        Ok(self)
    }
}

/// Result of publishing one output chunk to all current subscribers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PublishReport {
    /// Frames produced.
    pub frames: usize,
    /// Payload bytes published.
    pub bytes: usize,
    /// Subscribers detached because their bounded queue was full.
    pub detached_subscribers: usize,
}

/// Registry of terminal streams.
pub struct TerminalHub {
    limits: TerminalLimits,
    sessions: Mutex<HashMap<Uuid, Arc<TerminalSession>>>,
}

impl TerminalHub {
    /// Build a hub with validated limits.
    pub fn new(limits: TerminalLimits) -> Result<Self, TerminalHubError> {
        Ok(Self {
            limits: limits.validate()?,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    /// Limits used by this hub.
    pub fn limits(&self) -> TerminalLimits {
        self.limits
    }

    /// Register a new stream and return its identifier.
    pub fn create_session(&self) -> Result<Uuid, TerminalHubError> {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if sessions.len() >= self.limits.max_sessions {
            return Err(TerminalHubError::TooManySessions(self.limits.max_sessions));
        }
        let stream_id = loop {
            let candidate = Uuid::new_v4();
            if !sessions.contains_key(&candidate) {
                break candidate;
            }
        };
        sessions.insert(
            stream_id,
            Arc::new(TerminalSession::new(stream_id, self.limits)),
        );
        Ok(stream_id)
    }

    /// Look up a stream.
    pub fn session(&self, stream_id: Uuid) -> Option<Arc<TerminalSession>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&stream_id)
            .cloned()
    }

    /// Remove a stream and detach all subscribers.
    pub fn remove_session(&self, stream_id: Uuid) -> bool {
        let removed = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&stream_id);
        if let Some(session) = &removed {
            session.close();
        }
        removed.is_some()
    }

    /// Number of registered streams.
    pub fn session_count(&self) -> usize {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Subscribe to one stream with a bounded queue.
    pub fn subscribe(
        &self,
        stream_id: Uuid,
        queue_bytes: Option<u32>,
    ) -> Result<TerminalSubscription, TerminalHubError> {
        let session = self
            .session(stream_id)
            .ok_or(TerminalHubError::UnknownSession(stream_id))?;
        let queue = queue_bytes
            .map(|value| value as usize)
            .unwrap_or(self.limits.default_queue_bytes);
        if queue == 0 || queue > self.limits.max_queue_bytes {
            return Err(TerminalHubError::InvalidQueue {
                requested: queue,
                maximum: self.limits.max_queue_bytes,
            });
        }
        session.subscribe(queue)
    }

    /// Publish output bytes to all subscribers of a stream.
    pub fn publish(
        &self,
        stream_id: Uuid,
        bytes: &[u8],
    ) -> Result<PublishReport, TerminalHubError> {
        let session = self
            .session(stream_id)
            .ok_or(TerminalHubError::UnknownSession(stream_id))?;
        Ok(session.publish(bytes))
    }
}

/// One terminal stream and its bounded subscribers.
pub struct TerminalSession {
    stream_id: Uuid,
    limits: TerminalLimits,
    next_sequence: AtomicU64,
    subscribers: Mutex<Vec<Arc<TerminalSubscriber>>>,
    detached_total: AtomicUsize,
    published_bytes: AtomicUsize,
    closed: AtomicBool,
}

impl TerminalSession {
    fn new(stream_id: Uuid, limits: TerminalLimits) -> Self {
        Self {
            stream_id,
            limits,
            next_sequence: AtomicU64::new(1),
            subscribers: Mutex::new(Vec::new()),
            detached_total: AtomicUsize::new(0),
            published_bytes: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
        }
    }

    /// Mark the stream closed and detach every subscriber.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let mut subscribers = self
            .subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for subscriber in subscribers.drain(..) {
            subscriber.detach();
        }
    }

    /// Whether the stream has been closed.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Stream identifier.
    pub fn stream_id(&self) -> Uuid {
        self.stream_id
    }

    /// Number of currently attached subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Total subscribers detached for falling behind.
    pub fn detached_total(&self) -> usize {
        self.detached_total.load(Ordering::SeqCst)
    }

    /// Total payload bytes published.
    pub fn published_bytes(&self) -> usize {
        self.published_bytes.load(Ordering::SeqCst)
    }

    /// First sequence the next published frame will use.
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence.load(Ordering::SeqCst)
    }

    /// Publish bytes, splitting at the configured payload limit.
    pub fn publish(&self, bytes: &[u8]) -> PublishReport {
        let mut report = PublishReport::default();
        if bytes.is_empty() || self.is_closed() {
            return report;
        }
        for chunk in bytes.chunks(self.limits.max_payload_bytes) {
            let mut subscribers = self
                .subscribers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let sequence = self.next_sequence.fetch_add(1, Ordering::SeqCst);
            let frame = TerminalFrame::new(
                self.stream_id,
                TerminalFrameKind::Output,
                sequence,
                chunk.to_vec(),
            )
            .expect("published terminal frame is bounded and has a non-nil stream");
            let mut retained = Vec::with_capacity(subscribers.len());
            for subscriber in subscribers.drain(..) {
                if subscriber.push(frame.clone()) {
                    retained.push(subscriber);
                } else {
                    self.detached_total.fetch_add(1, Ordering::SeqCst);
                    report.detached_subscribers += 1;
                }
            }
            *subscribers = retained;
            report.frames += 1;
            report.bytes += chunk.len();
        }
        self.published_bytes
            .fetch_add(report.bytes, Ordering::SeqCst);
        report
    }

    fn subscribe(
        self: &Arc<Self>,
        queue_bytes: usize,
    ) -> Result<TerminalSubscription, TerminalHubError> {
        let subscriber = Arc::new(TerminalSubscriber::new(self.stream_id, queue_bytes));
        let mut subscribers = self
            .subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.is_closed() {
            return Err(TerminalHubError::Closed);
        }
        let initial_sequence = self.next_sequence.load(Ordering::SeqCst);
        subscribers.push(Arc::clone(&subscriber));
        drop(subscribers);
        Ok(TerminalSubscription {
            session: Arc::clone(self),
            subscriber,
            initial_sequence,
        })
    }

    fn remove_subscriber(&self, subscriber: &Arc<TerminalSubscriber>) {
        self.subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|candidate| !Arc::ptr_eq(candidate, subscriber));
    }
}

/// A subscriber's bounded output queue.
pub struct TerminalSubscriber {
    stream_id: Uuid,
    max_queue_bytes: usize,
    queue: Mutex<VecDeque<TerminalFrame>>,
    ready: Condvar,
    queued_bytes: AtomicUsize,
    detached: AtomicBool,
}

impl TerminalSubscriber {
    fn new(stream_id: Uuid, max_queue_bytes: usize) -> Self {
        Self {
            stream_id,
            max_queue_bytes,
            queue: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
            queued_bytes: AtomicUsize::new(0),
            detached: AtomicBool::new(false),
        }
    }

    fn push(&self, frame: TerminalFrame) -> bool {
        if self.detached.load(Ordering::SeqCst) {
            return false;
        }
        let frame_bytes = frame.encoded_len();
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let queued = self.queued_bytes.load(Ordering::SeqCst);
        if queued.saturating_add(frame_bytes) > self.max_queue_bytes {
            self.detached.store(true, Ordering::SeqCst);
            self.ready.notify_all();
            return false;
        }
        queue.push_back(frame);
        self.queued_bytes
            .store(queued + frame_bytes, Ordering::SeqCst);
        self.ready.notify_one();
        true
    }

    fn pop(&self) -> Option<TerminalFrame> {
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::pop_locked(&mut queue, &self.queued_bytes)
    }

    fn pop_locked(
        queue: &mut VecDeque<TerminalFrame>,
        queued_bytes: &AtomicUsize,
    ) -> Option<TerminalFrame> {
        let frame = queue.pop_front()?;
        let queued = queued_bytes.load(Ordering::SeqCst);
        queued_bytes.store(queued.saturating_sub(frame.encoded_len()), Ordering::SeqCst);
        Some(frame)
    }

    fn wait_next(&self, timeout: Duration) -> Option<TerminalFrame> {
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if let Some(frame) = Self::pop_locked(&mut queue, &self.queued_bytes) {
                return Some(frame);
            }
            if self.is_detached() {
                return None;
            }
            let (next, result) = self
                .ready
                .wait_timeout(queue, timeout)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            queue = next;
            if result.timed_out() && queue.is_empty() {
                return None;
            }
        }
    }

    fn detach(&self) {
        self.detached.store(true, Ordering::SeqCst);
        self.ready.notify_all();
    }

    /// Stream identifier.
    pub fn stream_id(&self) -> Uuid {
        self.stream_id
    }

    /// Whether this subscriber was detached for falling behind.
    pub fn is_detached(&self) -> bool {
        self.detached.load(Ordering::SeqCst)
    }

    /// Current queued bytes.
    pub fn queued_bytes(&self) -> usize {
        self.queued_bytes.load(Ordering::SeqCst)
    }

    /// Configured queue capacity in bytes.
    pub fn queue_capacity(&self) -> usize {
        self.max_queue_bytes
    }
}

/// Attached terminal subscription. Dropping it removes the subscriber.
pub struct TerminalSubscription {
    session: Arc<TerminalSession>,
    subscriber: Arc<TerminalSubscriber>,
    initial_sequence: u64,
}

impl TerminalSubscription {
    /// Stream identifier.
    pub fn stream_id(&self) -> Uuid {
        self.subscriber.stream_id()
    }

    /// Next queued frame, if available.
    pub fn try_next(&self) -> Option<TerminalFrame> {
        self.subscriber.pop()
    }

    /// Wait up to `timeout` for the next frame.
    pub fn wait_next(&self, timeout: Duration) -> Option<TerminalFrame> {
        self.subscriber.wait_next(timeout)
    }

    /// Whether the subscriber has been detached for falling behind.
    pub fn is_detached(&self) -> bool {
        self.subscriber.is_detached()
    }

    /// Current queued bytes.
    pub fn queued_bytes(&self) -> usize {
        self.subscriber.queued_bytes()
    }

    /// Configured queue capacity in bytes.
    pub fn queue_capacity(&self) -> usize {
        self.subscriber.queue_capacity()
    }

    /// First sequence the client should expect.
    pub fn next_sequence(&self) -> u64 {
        self.initial_sequence
    }
}

impl Drop for TerminalSubscription {
    fn drop(&mut self) {
        self.session.remove_subscriber(&self.subscriber);
    }
}

/// Terminal hub or subscription failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TerminalHubError {
    /// Hub limits are invalid.
    #[error("invalid terminal hub limits: {0}")]
    InvalidLimits(String),
    /// Maximum stream count reached.
    #[error("terminal hub session limit reached: {0}")]
    TooManySessions(usize),
    /// Requested stream does not exist.
    #[error("terminal stream {0} does not exist")]
    UnknownSession(Uuid),
    /// Requested queue is outside the configured bounds.
    #[error("terminal queue {requested} bytes is outside 1..={maximum}")]
    InvalidQueue {
        /// Requested queue bytes.
        requested: usize,
        /// Maximum allowed queue bytes.
        maximum: usize,
    },
    /// The stream was closed before subscription.
    #[error("terminal stream is closed")]
    Closed,
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };

    use super::*;

    #[test]
    fn bounded_slow_subscriber_is_detached_without_affecting_fast_reader() {
        let hub = Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap());
        let stream_id = hub.create_session().unwrap();
        let slow = hub.subscribe(stream_id, Some(64 * 1024)).unwrap();
        let fast = hub
            .subscribe(stream_id, Some(MAX_CLIENT_QUEUE_BYTES as u32))
            .unwrap();

        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut bytes = 0_usize;
            loop {
                if let Some(frame) = fast.try_next() {
                    bytes += frame.payload().len();
                    if bytes >= 10 * 1024 * 1024 {
                        break;
                    }
                } else if fast.is_detached() {
                    break;
                } else {
                    thread::yield_now();
                }
            }
            sender.send((bytes, fast.is_detached())).unwrap();
        });

        let chunk = vec![b'x'; MAX_TERMINAL_PAYLOAD];
        for _ in 0..160 {
            hub.publish(stream_id, &chunk).unwrap();
            thread::yield_now();
        }
        let (received, fast_detached) = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        reader.join().unwrap();

        assert_eq!(received, 10 * 1024 * 1024);
        assert!(!fast_detached);
        assert!(slow.is_detached());
        assert!(slow.queued_bytes() <= 64 * 1024);
        assert_eq!(hub.session(stream_id).unwrap().subscriber_count(), 0);
        assert_eq!(hub.session(stream_id).unwrap().detached_total(), 1);
    }

    #[test]
    fn late_subscriber_starts_at_the_next_stream_sequence() {
        let hub = TerminalHub::new(TerminalLimits::default()).unwrap();
        let stream_id = hub.create_session().unwrap();
        hub.publish(stream_id, b"before").unwrap();
        let subscription = hub.subscribe(stream_id, None).unwrap();
        assert_eq!(subscription.next_sequence(), 2);

        hub.publish(stream_id, b"after").unwrap();
        let frame = subscription.try_next().unwrap();
        assert_eq!(frame.sequence(), 2);
        assert_eq!(frame.payload(), b"after");
    }

    #[test]
    fn independent_streams_keep_independent_sequences() {
        let hub = TerminalHub::new(TerminalLimits::default()).unwrap();
        let first = hub.create_session().unwrap();
        let second = hub.create_session().unwrap();
        let first_subscription = hub.subscribe(first, None).unwrap();
        let second_subscription = hub.subscribe(second, None).unwrap();

        hub.publish(first, b"a").unwrap();
        hub.publish(second, b"b").unwrap();

        let first_frame = first_subscription.try_next().unwrap();
        let second_frame = second_subscription.try_next().unwrap();
        assert_eq!(first_frame.stream_id(), first);
        assert_eq!(second_frame.stream_id(), second);
        assert_eq!(first_frame.sequence(), 1);
        assert_eq!(second_frame.sequence(), 1);
    }

    #[test]
    fn detaching_a_slow_subscriber_does_not_stop_other_subscribers() {
        let hub = TerminalHub::new(TerminalLimits::default()).unwrap();
        let stream_id = hub.create_session().unwrap();
        let slow = hub.subscribe(stream_id, Some(1024)).unwrap();
        let fast = hub.subscribe(stream_id, Some(1024 * 1024)).unwrap();

        hub.publish(stream_id, &vec![1_u8; 8 * 1024]).unwrap();
        assert!(slow.is_detached());
        assert!(!fast.is_detached());
        assert!(fast.try_next().is_some());
    }

    #[test]
    fn removing_a_session_detaches_subscribers() {
        let hub = TerminalHub::new(TerminalLimits::default()).unwrap();
        let stream_id = hub.create_session().unwrap();
        let subscription = hub.subscribe(stream_id, None).unwrap();
        assert!(!subscription.is_detached());

        assert!(hub.remove_session(stream_id));
        assert!(subscription.is_detached());
        assert_eq!(hub.session_count(), 0);
        assert!(matches!(
            hub.subscribe(stream_id, None),
            Err(TerminalHubError::UnknownSession(_))
        ));
    }

    #[test]
    fn invalid_limits_and_queues_are_rejected() {
        assert!(matches!(
            TerminalHub::new(TerminalLimits {
                max_payload_bytes: MAX_TERMINAL_PAYLOAD + 1,
                ..TerminalLimits::default()
            }),
            Err(TerminalHubError::InvalidLimits(_))
        ));
        let hub = TerminalHub::new(TerminalLimits::default()).unwrap();
        let stream_id = hub.create_session().unwrap();
        assert!(matches!(
            hub.subscribe(stream_id, Some(0)),
            Err(TerminalHubError::InvalidQueue { .. })
        ));
        assert!(matches!(
            hub.subscribe(Uuid::new_v4(), None),
            Err(TerminalHubError::UnknownSession(_))
        ));
    }
}

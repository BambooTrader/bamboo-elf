use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use bamboo_core::{BusError, BusMessage, BusMetrics, BusReceiver, EventBus, Topic};
use tokio::sync::broadcast;

/// Default capacity for per-topic broadcast channels.
const CHANNEL_CAPACITY: usize = 1024;

/// Atomic counter set for a single topic.
struct TopicCounters {
    messages: AtomicU64,
}

/// Local in-process event bus backed by `tokio::broadcast` channels.
pub struct LocalBus {
    /// Per-topic broadcast senders.
    topics: RwLock<HashMap<Topic, broadcast::Sender<BusMessage>>>,
    /// Catch-all broadcast sender for `subscribe_all()`.
    all_tx: broadcast::Sender<BusMessage>,
    /// Global publish counter.
    total_published: AtomicU64,
    /// Per-topic counters.
    topic_counters: RwLock<HashMap<Topic, TopicCounters>>,
}

impl LocalBus {
    /// Create a new `LocalBus`. Pre-creates channels for every `Topic` variant.
    pub fn new() -> Self {
        let (all_tx, _) = broadcast::channel(CHANNEL_CAPACITY);

        let mut topics = HashMap::new();
        let mut counters = HashMap::new();

        for topic in Self::all_topics() {
            let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
            topics.insert(topic, tx);
            counters.insert(
                topic,
                TopicCounters {
                    messages: AtomicU64::new(0),
                },
            );
        }

        Self {
            topics: RwLock::new(topics),
            all_tx,
            total_published: AtomicU64::new(0),
            topic_counters: RwLock::new(counters),
        }
    }

    fn all_topics() -> Vec<Topic> {
        vec![
            Topic::MarketData,
            Topic::News,
            Topic::Signal,
            Topic::Intent,
            Topic::Risk,
            Topic::Execution,
            Topic::System,
        ]
    }
}

impl Default for LocalBus {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl EventBus for LocalBus {
    async fn publish(&self, msg: BusMessage) -> Result<usize, BusError> {
        let topic = msg.topic;

        // Send to the topic-specific channel.
        let topic_count = {
            let topics = self.topics.read().unwrap();
            match topics.get(&topic) {
                Some(tx) => match tx.send(msg.clone()) {
                    Ok(n) => n,
                    Err(_) => 0, // no receivers — that is fine
                },
                None => 0,
            }
        };

        // Send to the all-channel (best effort).
        let _ = self.all_tx.send(msg);

        // Update metrics.
        self.total_published.fetch_add(1, Ordering::Relaxed);
        {
            let counters = self.topic_counters.read().unwrap();
            if let Some(c) = counters.get(&topic) {
                c.messages.fetch_add(1, Ordering::Relaxed);
            }
        }

        Ok(topic_count)
    }

    fn subscribe(&self, topic: Topic) -> BusReceiver {
        let topics = self.topics.read().unwrap();
        topics
            .get(&topic)
            .expect("topic channel must exist")
            .subscribe()
    }

    fn subscribe_all(&self) -> BusReceiver {
        self.all_tx.subscribe()
    }

    fn metrics(&self) -> BusMetrics {
        let counters = self.topic_counters.read().unwrap();
        let topics = self.topics.read().unwrap();

        let mut messages_per_topic = HashMap::new();
        let mut queue_depth = HashMap::new();

        for topic in Self::all_topics() {
            if let Some(c) = counters.get(&topic) {
                messages_per_topic.insert(topic, c.messages.load(Ordering::Relaxed));
            }
            if let Some(tx) = topics.get(&topic) {
                queue_depth.insert(topic, tx.len());
            }
        }

        BusMetrics {
            messages_published: self.total_published.load(Ordering::Relaxed),
            messages_per_topic,
            queue_depth,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bamboo_core::{ComponentId, Payload, Topic};
    use uuid::Uuid;

    fn make_msg(topic: Topic) -> BusMessage {
        BusMessage {
            id: Uuid::new_v4(),
            topic,
            payload: Payload::NewsItem(bamboo_core::NewsItem {
                title: "test".into(),
                source: "test".into(),
                url: None,
                related_instruments: vec![],
                timestamp: 0,
            }),
            timestamp: 0,
            source: ComponentId::new("test"),
        }
    }

    #[tokio::test]
    async fn publish_subscribe_single_topic() {
        let bus = LocalBus::new();
        let mut rx = bus.subscribe(Topic::News);

        let msg = make_msg(Topic::News);
        let count = bus.publish(msg.clone()).await.unwrap();
        assert_eq!(count, 1);

        let received = rx.recv().await.unwrap();
        assert_eq!(received.id, msg.id);
    }

    #[tokio::test]
    async fn subscribe_all_receives_all_topics() {
        let bus = LocalBus::new();
        let mut rx_all = bus.subscribe_all();

        let msg1 = make_msg(Topic::News);
        let msg2 = make_msg(Topic::MarketData);

        bus.publish(msg1.clone()).await.unwrap();
        bus.publish(msg2.clone()).await.unwrap();

        let r1 = rx_all.recv().await.unwrap();
        let r2 = rx_all.recv().await.unwrap();
        assert_eq!(r1.id, msg1.id);
        assert_eq!(r2.id, msg2.id);
    }

    #[tokio::test]
    async fn metrics_are_tracked() {
        let bus = LocalBus::new();
        let _rx = bus.subscribe(Topic::Signal);

        bus.publish(make_msg(Topic::Signal)).await.unwrap();
        bus.publish(make_msg(Topic::Signal)).await.unwrap();
        bus.publish(make_msg(Topic::News)).await.unwrap();

        let m = bus.metrics();
        assert_eq!(m.messages_published, 3);
        assert_eq!(*m.messages_per_topic.get(&Topic::Signal).unwrap(), 2);
        assert_eq!(*m.messages_per_topic.get(&Topic::News).unwrap(), 1);
    }

    #[tokio::test]
    async fn publish_with_no_subscribers_returns_zero() {
        let bus = LocalBus::new();
        // No subscriber — should still succeed with count 0.
        let count = bus.publish(make_msg(Topic::MarketData)).await.unwrap();
        assert_eq!(count, 0);
    }
}

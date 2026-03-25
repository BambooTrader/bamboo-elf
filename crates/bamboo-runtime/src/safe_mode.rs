//! Safe Mode — emergency coordination for the trading system.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bamboo_core::{
    BusMessage, ComponentId, EmergencyAction, EmergencyActionType, EventBus, Payload, Topic,
};
use uuid::Uuid;

/// Safe mode coordination primitive.
///
/// When activated, the system stops all new order submissions, cancels pending orders,
/// and requires manual intervention to resume.
pub struct SafeMode {
    active: AtomicBool,
    reason: Mutex<String>,
    activated_at: AtomicU64,
}

impl SafeMode {
    pub fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            reason: Mutex::new(String::new()),
            activated_at: AtomicU64::new(0),
        }
    }

    /// Activate safe mode with a reason. Publishes EmergencyAction to the bus.
    pub async fn activate(&self, reason: &str, bus: &Arc<dyn EventBus>) {
        let was_active = self.active.swap(true, Ordering::SeqCst);
        if was_active {
            tracing::warn!("SafeMode already active, updating reason");
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        self.activated_at.store(now, Ordering::SeqCst);
        {
            let mut r = self.reason.lock().unwrap();
            *r = reason.to_string();
        }

        tracing::error!(reason, "SAFE MODE ACTIVATED");

        // Publish EmergencyAction.
        let emergency = EmergencyAction {
            id: Uuid::new_v4(),
            action_type: EmergencyActionType::HaltNewOrders,
            reason: reason.to_string(),
            affected_instruments: vec![],
            timestamp: now,
        };
        let msg = BusMessage {
            id: Uuid::new_v4(),
            topic: Topic::Risk,
            payload: Payload::EmergencyAction(emergency),
            timestamp: now,
            source: ComponentId::new("SafeMode"),
        };
        let _ = bus.publish(msg).await;
    }

    /// Deactivate safe mode.
    pub fn deactivate(&self) {
        self.active.store(false, Ordering::SeqCst);
        {
            let mut r = self.reason.lock().unwrap();
            r.clear();
        }
        self.activated_at.store(0, Ordering::SeqCst);
        tracing::info!("SafeMode deactivated");
    }

    /// Check if safe mode is currently active.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Get the reason for safe mode activation.
    pub fn reason(&self) -> String {
        self.reason.lock().unwrap().clone()
    }

    /// Get the timestamp when safe mode was activated (nanos since epoch).
    pub fn activated_at(&self) -> u64 {
        self.activated_at.load(Ordering::SeqCst)
    }
}

impl Default for SafeMode {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::LocalBus;

    #[tokio::test]
    async fn safe_mode_activate_deactivate() {
        let bus: Arc<dyn EventBus> = Arc::new(LocalBus::new());
        let sm = SafeMode::new();

        assert!(!sm.is_active());
        assert!(sm.reason().is_empty());

        sm.activate("Test drawdown breach", &bus).await;
        assert!(sm.is_active());
        assert_eq!(sm.reason(), "Test drawdown breach");
        assert!(sm.activated_at() > 0);

        sm.deactivate();
        assert!(!sm.is_active());
        assert!(sm.reason().is_empty());
        assert_eq!(sm.activated_at(), 0);
    }

    #[tokio::test]
    async fn safe_mode_publishes_emergency() {
        let bus: Arc<dyn EventBus> = Arc::new(LocalBus::new());
        let mut rx = bus.subscribe(Topic::Risk);
        let sm = SafeMode::new();

        sm.activate("Kill switch", &bus).await;

        // Should have received an EmergencyAction message.
        let msg = rx.try_recv();
        assert!(msg.is_ok(), "Expected emergency message on bus");
        if let Payload::EmergencyAction(ea) = &msg.unwrap().payload {
            assert_eq!(ea.action_type, EmergencyActionType::HaltNewOrders);
            assert_eq!(ea.reason, "Kill switch");
        } else {
            panic!("Expected EmergencyAction payload");
        }
    }

    #[test]
    fn safe_mode_default() {
        let sm = SafeMode::default();
        assert!(!sm.is_active());
    }
}

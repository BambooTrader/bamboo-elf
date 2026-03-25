use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

/// Shared shutdown coordination primitive.
///
/// Tasks check `is_shutdown()` in their loops and can `wait_for_shutdown().await`
/// to be notified asynchronously.
#[derive(Clone)]
pub struct ShutdownSignal {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl ShutdownSignal {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Signal shutdown. Sets the flag and wakes all waiters.
    pub fn shutdown(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    /// Backwards-compatible alias for `shutdown()`.
    pub fn trigger(&self) {
        self.shutdown();
    }

    /// Returns `true` if shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Wait until shutdown is requested. Returns immediately if already set.
    pub async fn wait_for_shutdown(&self) {
        if self.is_shutdown() {
            return;
        }
        self.notify.notified().await;
    }
}

impl Default for ShutdownSignal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_flag() {
        let sig = ShutdownSignal::new();
        assert!(!sig.is_shutdown());
        sig.shutdown();
        assert!(sig.is_shutdown());
    }

    #[tokio::test]
    async fn wait_completes_after_shutdown() {
        let sig = ShutdownSignal::new();
        let sig2 = sig.clone();

        let handle = tokio::spawn(async move {
            sig2.wait_for_shutdown().await;
            true
        });

        // Give the spawned task a moment to start waiting.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        sig.shutdown();

        let result = handle.await.unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn wait_returns_immediately_if_already_shutdown() {
        let sig = ShutdownSignal::new();
        sig.shutdown();
        // Should not hang.
        sig.wait_for_shutdown().await;
        assert!(sig.is_shutdown());
    }
}

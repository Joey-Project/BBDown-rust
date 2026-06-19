use crate::{Error, Result};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::watch;

const DEFAULT_CANCEL_REASON: &str = "download cancelled";

#[derive(Clone, Debug)]
pub struct DownloadCancellationToken {
    state: Arc<DownloadCancellationState>,
}

#[derive(Debug)]
struct DownloadCancellationState {
    cancelled: AtomicBool,
    reason: Mutex<Option<String>>,
    cancelled_tx: watch::Sender<bool>,
}

impl DownloadCancellationToken {
    #[must_use]
    pub fn new() -> Self {
        let (cancelled_tx, _) = watch::channel(false);
        Self {
            state: Arc::new(DownloadCancellationState {
                cancelled: AtomicBool::new(false),
                reason: Mutex::new(None),
                cancelled_tx,
            }),
        }
    }

    pub fn cancel(&self) {
        self.cancel_with_reason(DEFAULT_CANCEL_REASON);
    }

    pub fn cancel_with_reason(&self, reason: impl Into<String>) {
        if self.state.cancelled.load(Ordering::SeqCst) {
            return;
        }
        let Ok(mut stored) = self.state.reason.lock() else {
            if self
                .state
                .cancelled
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                self.state.cancelled_tx.send_replace(true);
            }
            return;
        };
        if !self.state.cancelled.load(Ordering::SeqCst) {
            *stored = Some(reason.into());
            self.state.cancelled.store(true, Ordering::SeqCst);
            self.state.cancelled_tx.send_replace(true);
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn reason(&self) -> Option<String> {
        self.state
            .reason
            .lock()
            .ok()
            .and_then(|reason| reason.clone())
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let mut cancelled_rx = self.state.cancelled_tx.subscribe();
        if *cancelled_rx.borrow() {
            return;
        }
        while cancelled_rx.changed().await.is_ok() {
            if *cancelled_rx.borrow() {
                return;
            }
        }
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            return Err(self.cancelled_error());
        }
        Ok(())
    }

    #[must_use]
    pub fn cancelled_error(&self) -> Error {
        Error::Cancelled {
            reason: self
                .reason()
                .unwrap_or_else(|| DEFAULT_CANCEL_REASON.to_owned()),
        }
    }
}

impl Default for DownloadCancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::DownloadCancellationToken;
    use std::time::Duration;

    #[tokio::test]
    async fn cancelled_returns_for_late_subscriber() {
        let cancellation = DownloadCancellationToken::new();
        cancellation.cancel();

        let result =
            tokio::time::timeout(Duration::from_millis(100), cancellation.cancelled()).await;
        assert!(
            result.is_ok(),
            "pre-cancelled token should notify late subscribers"
        );
    }

    #[tokio::test]
    async fn cancelled_returns_for_waiting_subscriber() {
        let cancellation = DownloadCancellationToken::new();
        let waiting_cancellation = cancellation.clone();
        let waiter = tokio::spawn(async move {
            waiting_cancellation.cancelled().await;
        });

        cancellation.cancel_with_reason("test cancellation");

        let result = tokio::time::timeout(Duration::from_millis(100), waiter).await;
        assert!(result.is_ok(), "waiting token should be notified");
        if let Ok(join_result) = result {
            assert!(join_result.is_ok(), "waiter task should finish");
        }
    }

    #[test]
    fn cancellation_reason_is_available_after_cancel() {
        let cancellation = DownloadCancellationToken::new();

        cancellation.cancel_with_reason("custom cancellation reason");

        assert_eq!(
            cancellation.reason().as_deref(),
            Some("custom cancellation reason")
        );
        assert_eq!(
            cancellation.cancelled_error().to_string(),
            "download cancelled: custom cancellation reason"
        );
    }
}

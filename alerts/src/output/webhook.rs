//! Webhook notification support for swap alerts.
//!
//! This module provides asynchronous webhook delivery for swap events,
//! with retry logic and backoff for reliability.

use {
    super::SwapEvent,
    std::{env, sync::Arc, time::Duration},
    tokio::sync::mpsc,
};

/// Configuration for webhook notifications.
#[derive(Debug, Clone)]
pub struct WebhookConfig {
    /// Webhook URL to POST events to
    pub url: String,
    /// Request timeout
    pub timeout: Duration,
    /// Maximum retry attempts for failed deliveries
    pub max_retries: u32,
    /// Initial backoff duration between retries
    pub retry_backoff: Duration,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            timeout: Duration::from_secs(10),
            max_retries: 3,
            retry_backoff: Duration::from_millis(500),
        }
    }
}

impl WebhookConfig {
    /// Creates a new webhook configuration from environment variables.
    ///
    /// # Environment Variables
    ///
    /// - `WEBHOOK_URL` - Required: The URL to POST events to
    /// - `WEBHOOK_TIMEOUT_SECS` - Optional: Request timeout in seconds (default: 10)
    /// - `WEBHOOK_MAX_RETRIES` - Optional: Max retry attempts (default: 3)
    /// - `WEBHOOK_RETRY_BACKOFF_MS` - Optional: Initial backoff in ms (default: 500)
    ///
    /// # Returns
    ///
    /// `Some(WebhookConfig)` if `WEBHOOK_URL` is set, `None` otherwise.
    pub fn from_env() -> Option<Self> {
        let url = env::var("WEBHOOK_URL").ok()?;
        if url.trim().is_empty() {
            return None;
        }

        let timeout_secs: u64 = env::var("WEBHOOK_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let max_retries: u32 = env::var("WEBHOOK_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);

        let retry_backoff_ms: u64 = env::var("WEBHOOK_RETRY_BACKOFF_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(500);

        Some(Self {
            url,
            timeout: Duration::from_secs(timeout_secs),
            max_retries,
            retry_backoff: Duration::from_millis(retry_backoff_ms),
        })
    }
}

/// Asynchronous webhook notifier that delivers swap events to a configured endpoint.
///
/// Uses a background task with a channel to decouple event production from delivery,
/// preventing webhook latency from blocking swap processing.
///
/// # Example
///
/// ```ignore
/// let config = WebhookConfig {
///     url: "https://example.com/webhook".to_string(),
///     ..Default::default()
/// };
/// let notifier = WebhookNotifier::new(config);
///
/// // Send events (non-blocking)
/// notifier.send(swap_event).await;
///
/// // Graceful shutdown
/// notifier.shutdown().await;
/// ```
pub struct WebhookNotifier {
    /// Channel sender for queuing events
    tx: mpsc::Sender<SwapEvent>,
    /// Handle to the background delivery task
    _task_handle: tokio::task::JoinHandle<()>,
}

impl WebhookNotifier {
    /// Creates a new webhook notifier with the given configuration.
    ///
    /// Spawns a background task that processes the event queue and delivers
    /// events to the configured webhook URL.
    ///
    /// # Arguments
    ///
    /// * `config` - Webhook configuration including URL and retry settings
    pub fn new(config: WebhookConfig) -> Self {
        // Channel buffer size: 1000 events should handle burst traffic
        // If the buffer fills, send() will block until space is available
        let (tx, rx) = mpsc::channel::<SwapEvent>(1000);
        let config = Arc::new(config);

        let task_handle = tokio::spawn(Self::delivery_task(rx, config));

        Self {
            tx,
            _task_handle: task_handle,
        }
    }

    /// Queues a swap event for webhook delivery.
    ///
    /// This is non-blocking unless the internal buffer is full.
    /// Events are delivered asynchronously by the background task.
    ///
    /// # Arguments
    ///
    /// * `event` - The swap event to deliver
    ///
    /// # Returns
    ///
    /// `Ok(())` if queued successfully, `Err` if the channel is closed.
    #[allow(dead_code)]
    pub async fn send(&self, event: SwapEvent) -> Result<(), mpsc::error::SendError<SwapEvent>> {
        self.tx.send(event).await
    }

    /// Tries to queue a swap event without blocking.
    ///
    /// # Arguments
    ///
    /// * `event` - The swap event to deliver
    ///
    /// # Returns
    ///
    /// `Ok(())` if queued successfully, `Err` if the channel is full or closed.
    #[allow(clippy::result_large_err)]
    pub fn try_send(&self, event: SwapEvent) -> Result<(), mpsc::error::TrySendError<SwapEvent>> {
        self.tx.try_send(event)
    }

    /// Background task that processes the event queue and delivers webhooks.
    async fn delivery_task(mut rx: mpsc::Receiver<SwapEvent>, config: Arc<WebhookConfig>) {
        // Create HTTP client with timeout
        // Note: reqwest is not in dependencies, so we use a simple approach
        // For production, add reqwest and use it instead
        let client = match reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                log::error!("Failed to create HTTP client for webhooks: {e}");
                return;
            }
        };

        while let Some(event) = rx.recv().await {
            let json = match serde_json::to_string(&event) {
                Ok(j) => j,
                Err(e) => {
                    log::error!("Failed to serialize swap event: {e}");
                    continue;
                }
            };

            // Retry loop with exponential backoff
            let mut attempt = 0;
            let mut backoff = config.retry_backoff;

            loop {
                attempt += 1;
                match client
                    .post(&config.url)
                    .header("Content-Type", "application/json")
                    .body(json.clone())
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        log::debug!(
                            "Webhook delivered: sig={}, status={}",
                            event.signature,
                            resp.status()
                        );
                        break;
                    }
                    Ok(resp) => {
                        log::warn!(
                            "Webhook failed: sig={}, status={}, attempt={}/{}",
                            event.signature,
                            resp.status(),
                            attempt,
                            config.max_retries + 1
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "Webhook error: sig={}, err={e}, attempt={}/{}",
                            event.signature,
                            attempt,
                            config.max_retries + 1
                        );
                    }
                }

                if attempt > config.max_retries {
                    log::error!(
                        "Webhook delivery failed after {} attempts: sig={}",
                        attempt,
                        event.signature
                    );
                    break;
                }

                // Exponential backoff
                tokio::time::sleep(backoff).await;
                backoff *= 2;
            }
        }

        log::info!("Webhook delivery task shutting down");
    }

    /// Returns the number of events currently queued for delivery.
    #[allow(dead_code)]
    pub fn queue_len(&self) -> usize {
        // capacity() - permits available = current queue size
        // Note: This is an approximation as the channel may change between calls
        1000 - self.tx.capacity()
    }

    /// Returns true if the webhook queue is empty.
    #[allow(dead_code)]
    pub fn is_queue_empty(&self) -> bool {
        self.tx.capacity() == 1000
    }
}

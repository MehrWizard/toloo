/// Async batch processing pipeline for high-throughput request handling.
///
/// Accumulates incoming requests via a bounded channel and processes them
/// in batches to reduce lock contention and syscall overhead.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::{self, error::TrySendError, Receiver, Sender};
use tokio::sync::oneshot;
use tokio::time::{timeout_at, Instant};

use crate::envelope::{innermost, verify_chain, verify_chains_batch};
use crate::pool::Pool;
use crate::requests::handlers::{error_bytes, handle_request, handle_request_verified};
use crate::transport::server::RelayConfig;

use crate::types::Envelope;

/// Maximum batch size for request accumulation.
const MAX_BATCH_SIZE: usize = 100;
/// Maximum time to wait before flushing a partial batch.
const BATCH_FLUSH_INTERVAL: Duration = Duration::from_millis(5);
/// Bound on queued requests before callers fall back to direct processing.
const CHANNEL_CAPACITY: usize = 10_000;

/// A request to be processed by the batch processor.
struct BatchRequest {
    /// The envelope to process.
    envelope: Envelope,
    /// Response channel for the result.
    response_tx: oneshot::Sender<anyhow::Result<Vec<Vec<u8>>>>,
    /// Pool reference for processing.
    pool: Arc<Pool>,
    /// Config reference for processing.
    config: Arc<RelayConfig>,
}

/// Batch processor that accumulates requests and processes them in batches.
pub struct BatchProcessor {
    /// Channel for submitting requests to the batch processor.
    request_tx: Sender<BatchRequest>,
    /// Receiver, taken by [`BatchProcessor::start`] when the task is spawned.
    request_rx: Mutex<Option<Receiver<BatchRequest>>>,
}

impl BatchProcessor {
    /// Create a new batch processor. The background task is not spawned until
    /// [`BatchProcessor::start`] is called from within a `LocalSet`.
    pub fn new(_pool: Arc<Pool>, _config: Arc<RelayConfig>) -> Self {
        let (request_tx, request_rx) = mpsc::channel(CHANNEL_CAPACITY);
        Self {
            request_tx,
            request_rx: Mutex::new(Some(request_rx)),
        }
    }

    /// Spawn the background processing task on the current `LocalSet`.
    ///
    /// Must be called from inside a `task::LocalSet`; `spawn_local` panics
    /// otherwise. Idempotent — only the first call starts the task.
    pub fn start(&self) {
        let rx = self
            .request_rx
            .lock()
            .expect("batch processor lock poisoned")
            .take();
        if let Some(rx) = rx {
            tokio::task::spawn_local(run_batch_processor(rx));
        }
    }

    /// Submit a request for batch processing.
    ///
    /// Returns a receiver that will get the response once processed.
    pub async fn submit(
        &self,
        envelope: Envelope,
        pool: Arc<Pool>,
        config: Arc<RelayConfig>,
    ) -> anyhow::Result<Vec<Vec<u8>>> {
        let (tx, rx) = oneshot::channel();

        let req = BatchRequest {
            envelope,
            response_tx: tx,
            pool,
            config,
        };

        // Try to send, but if the channel is full or closed, fall back to
        // direct processing rather than blocking or dropping the request.
        match self.request_tx.try_send(req) {
            Ok(()) => {}
            Err(TrySendError::Full(req)) | Err(TrySendError::Closed(req)) => {
                return handle_request(req.envelope, &req.pool, &req.config).await;
            }
        }

        // Wait for the batch processor to complete the request
        rx.await.map_err(|_| anyhow::anyhow!("batch processor dropped"))?
    }
}

/// Main batch processing loop.
///
/// Blocks on the channel when idle (no busy-waiting) and accumulates up to
/// [`MAX_BATCH_SIZE`] requests or until [`BATCH_FLUSH_INTERVAL`] elapses after
/// the first request of a batch.
async fn run_batch_processor(mut request_rx: Receiver<BatchRequest>) {
    let mut batch = Vec::with_capacity(MAX_BATCH_SIZE);

    loop {
        let Some(first) = request_rx.recv().await else {
            break;
        };
        batch.push(first);

        let deadline = Instant::now() + BATCH_FLUSH_INTERVAL;
        while batch.len() < MAX_BATCH_SIZE {
            match timeout_at(deadline, request_rx.recv()).await {
                Ok(Some(req)) => batch.push(req),
                Ok(None) | Err(_) => break,
            }
        }

        process_batch(std::mem::take(&mut batch)).await;
    }
}

/// Process a batch of requests.
async fn process_batch(batch: Vec<BatchRequest>) {
    if batch.is_empty() {
        return;
    }

    // Verify all request envelopes together. A single request keeps the fast
    // single-signature path; two or more use one Pippenger batch.
    let verdicts = {
        let refs: Vec<&Envelope> = batch.iter().map(|req| &req.envelope).collect();
        if refs.len() >= 2 {
            verify_chains_batch(&refs)
        } else {
            refs.iter().map(|env| verify_chain(env)).collect()
        }
    };

    let mut verified = Vec::with_capacity(batch.len());
    for (req, verdict) in batch.into_iter().zip(verdicts) {
        match verdict {
            Ok(()) => verified.push(req),
            Err(e) => {
                let _ = req
                    .response_tx
                    .send(Ok(vec![error_bytes("invalid_signature", &e.to_string())]));
            }
        }
    }

    // Group by request type for specialized batch handling
    let mut pool_exchange_requests = Vec::new();
    let mut other_requests = Vec::new();

    for req in verified {
        match innermost(&req.envelope).d.t.as_str() {
            "pool.exchange" => pool_exchange_requests.push(req),
            _ => other_requests.push(req),
        }
    }

    // Process pool.exchange requests with batch verification + batch put
    if !pool_exchange_requests.is_empty() {
        process_pool_exchange_batch(pool_exchange_requests).await;
    }

    // Dispatch already-verified requests without re-checking request signatures.
    for req in other_requests {
        let result = handle_request_verified(req.envelope, &req.pool, &req.config).await;
        let _ = req.response_tx.send(result);
    }
}

/// Process a batch of pool.exchange requests with optimized batch verification and put.
async fn process_pool_exchange_batch(requests: Vec<BatchRequest>) {
    // Request signatures are already verified as one batch above; offers are
    // batch-verified per request inside the handler.
    // Future optimization: merge offer arrays across requests for mega-batch.
    for req in requests {
        let result = handle_request_verified(req.envelope, &req.pool, &req.config).await;
        let _ = req.response_tx.send(result);
    }
}

/// Submit a single request through the batch processor if available,
/// otherwise process directly.
pub async fn try_batch_or_direct(
    processor: Option<&BatchProcessor>,
    envelope: Envelope,
    pool: Arc<Pool>,
    config: Arc<RelayConfig>,
) -> anyhow::Result<Vec<Vec<u8>>> {
    if let Some(proc) = processor {
        proc.submit(envelope, pool, config).await
    } else {
        handle_request(envelope, &pool, &config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::make_envelope;
    use serde_json::json;
    use std::collections::HashMap;
    use tokio::task::LocalSet;

    #[tokio::test(flavor = "current_thread")]
    async fn batch_processor_verifies_multiple_requests_together() {
        let pool = Arc::new(Pool::memory().unwrap());
        let node = crate::fixtures::mk_node();
        let config = Arc::new(RelayConfig {
            node: node.clone(),
            endpoints: vec![],
            pool: Arc::clone(&pool),
            rooms: HashMap::new(),
            metrics: None,
            batch_processor: None,
        });
        let bp = BatchProcessor::new(Arc::clone(&pool), Arc::clone(&config));

        let local = LocalSet::new();
        local
            .run_until(async {
                bp.start();
                let mut pending = Vec::new();
                for _ in 0..8 {
                    let req = make_envelope(
                        "node.peers",
                        Some(json!({ "limit": 0 })),
                        &node,
                        None,
                    )
                    .expect("request envelope");
                    pending.push(bp.submit(req, Arc::clone(&pool), Arc::clone(&config)));
                }
                for result in futures_util::future::join_all(pending).await {
                    let responses = result.expect("submit should succeed");
                    assert!(responses.is_empty(), "limit 0 returns no frames");
                }
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn batch_processor_starts_inside_local_set_and_processes() {
        let pool = Arc::new(Pool::memory().unwrap());
        let node = crate::fixtures::mk_node();
        let config = Arc::new(RelayConfig {
            node: node.clone(),
            endpoints: vec![],
            pool: Arc::clone(&pool),
            rooms: HashMap::new(),
            metrics: None,
            batch_processor: None,
        });
        let bp = BatchProcessor::new(Arc::clone(&pool), Arc::clone(&config));

        let local = LocalSet::new();
        local
            .run_until(async {
                bp.start();
                let req = make_envelope(
                    "unknown.endpoint",
                    Some(json!({})),
                    &node,
                    None,
                )
                .expect("request envelope");
                let resp = bp
                    .submit(req, Arc::clone(&pool), Arc::clone(&config))
                    .await
                    .expect("submit should succeed");
                assert_eq!(resp.len(), 1, "unknown endpoint yields one error frame");
                assert!(!resp[0].is_empty());
            })
            .await;
    }
}

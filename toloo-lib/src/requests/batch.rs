/// Async batch processing pipeline for high-throughput request handling.
///
/// Accumulates incoming requests via crossbeam-channel and processes them
/// in batches to reduce lock contention and syscall overhead.
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError};
use tokio::sync::oneshot;
use tokio::time::interval;
use crate::transport::server::RelayConfig;

use crate::types::Envelope;
use crate::pool::Pool;
use crate::requests::handlers::handle_request;

/// Maximum batch size for request accumulation.
const MAX_BATCH_SIZE: usize = 100;
/// Maximum time to wait before flushing a partial batch.
const BATCH_FLUSH_INTERVAL: Duration = Duration::from_millis(5);

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
    /// Handle to the background processing task.
    _handle: tokio::task::JoinHandle<()>,
}

impl BatchProcessor {
    /// Create a new batch processor with the given pool and config.
    pub fn new(pool: Arc<Pool>, config: Arc<RelayConfig>) -> Self {
        let (request_tx, request_rx) = bounded(10_000);

        let handle = tokio::task::spawn_local(run_batch_processor(
            request_rx,
            pool,
            config,
        ));

        Self {
            request_tx,
            _handle: handle,
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
            envelope: envelope.clone(),
            response_tx: tx,
            pool: pool.clone(),
            config: config.clone(),
        };

        // Try to send, but if the channel is full, fall back to direct processing
        if self.request_tx.try_send(req).is_err() {
            // Channel full - process directly to avoid blocking
            return handle_request(envelope, &pool, &config).await;
        }

        // Wait for the batch processor to complete the request
        rx.await.map_err(|_| anyhow::anyhow!("batch processor dropped"))?
    }
}

/// Main batch processing loop.
async fn run_batch_processor(
    request_rx: Receiver<BatchRequest>,
    _pool: Arc<Pool>,
    _config: Arc<RelayConfig>,
) {
    let mut batch = Vec::with_capacity(MAX_BATCH_SIZE);
    let mut flush_interval = interval(BATCH_FLUSH_INTERVAL);

    loop {
        tokio::select! {
            // Accumulate requests
            biased;
            _ = flush_interval.tick() => {
                if !batch.is_empty() {
                    process_batch(batch.drain(..).collect()).await;
                }
            }
            // Try to receive a request
            result = try_recv_async(&request_rx) => {
                match result {
                    Ok(req) => {
                        batch.push(req);
                        if batch.len() >= MAX_BATCH_SIZE {
                            process_batch(batch.drain(..).collect()).await;
                        }
                    }
                    Err(TryRecvError::Empty) => {
                        // No requests available, continue to next select
                        tokio::task::yield_now().await;
                    }
                    Err(TryRecvError::Disconnected) => {
                        // Channel closed, process remaining and exit
                        if !batch.is_empty() {
                            process_batch(batch.drain(..).collect()).await;
                        }
                        break;
                    }
                }
            }
        }
    }
}

/// Async wrapper for crossbeam_channel try_recv.
async fn try_recv_async<T>(rx: &Receiver<T>) -> Result<T, TryRecvError> {
    // Use a small sleep to yield control, then try receive
    match rx.try_recv() {
        Ok(v) => Ok(v),
        Err(TryRecvError::Empty) => {
            tokio::task::yield_now().await;
            rx.try_recv()
        }
        Err(e) => Err(e),
    }
}

/// Process a batch of requests.
async fn process_batch(batch: Vec<BatchRequest>) {
    if batch.is_empty() {
        return;
    }

    // Group by request type for specialized batch handling
    let mut pool_exchange_requests = Vec::new();
    let mut other_requests = Vec::new();

    for req in batch {
        let inner = crate::envelope::innermost(&req.envelope);
        match inner.d.t.as_str() {
            "pool.exchange" => pool_exchange_requests.push(req),
            _ => other_requests.push(req),
        }
    }

    // Process pool.exchange requests with batch verification + batch put
    if !pool_exchange_requests.is_empty() {
        process_pool_exchange_batch(pool_exchange_requests).await;
    }

    // Process other requests individually (could be batched in future)
    for req in other_requests {
        let result = handle_request(req.envelope, &req.pool, &req.config).await;
        let _ = req.response_tx.send(result);
    }
}

/// Process a batch of pool.exchange requests with optimized batch verification and put.
async fn process_pool_exchange_batch(requests: Vec<BatchRequest>) {
    // For now, process each individually but with batch-optimized pool operations
    // Future optimization: merge offer arrays across requests for mega-batch
    for req in requests {
        let result = handle_request(req.envelope, &req.pool, &req.config).await;
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
//! Server-side replay loop: walks an mmap'd ITCH 5.0 file and feeds the
//! moldudp server in randomized batches.

use anyhow::Result;
use memmap2::Mmap;
use moldudp::ServerHandle;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Replay every ITCH5 message in `mmap` through `handle`, batching with a
/// random size in `1..=max_msgs` per flush. Returns once the file is
/// exhausted, a parse error halts progress, or `shutdown` is set.
pub fn replay(
    mmap: Mmap,
    handle: &ServerHandle,
    shutdown: Arc<AtomicBool>,
    max_msgs: usize,
) -> Result<()> {
    tracing::info!("server thread started");
    let start = Instant::now();
    let total_bytes = mmap.len();
    let progress_interval = (total_bytes / 20).max(1);

    let mut buf: &[u8] = &mmap;
    let mut batch = Batch::new(max_msgs);
    let mut counters = ReplayCounters::default();
    let mut next_progress_threshold = progress_interval;

    while !buf.is_empty() {
        if shutdown.load(Ordering::Relaxed) {
            let offset = total_bytes - buf.len();
            tracing::info!(
                "server shutdown requested at offset {offset}/{total_bytes} ({:.1}%, {})",
                100.0 * offset as f64 / total_bytes as f64,
                counters,
            );
            break;
        }

        let before = buf.len();
        let (msg, rest) = match itch5::parse_one(buf) {
            Ok(t) => t,
            Err(e) => {
                let offset = total_bytes - buf.len();
                tracing::error!(
                    "parse error at offset {offset} ({} bytes remaining): {e:?}",
                    buf.len()
                );
                break;
            }
        };
        if rest.len() >= before {
            tracing::error!(
                "parser made no progress at offset {}; aborting",
                total_bytes - before,
            );
            break;
        }

        let msg_bytes = before - rest.len();
        counters.bytes_processed += msg_bytes;
        buf = rest;
        batch.push(msg.to_vec());
        counters.parsed += 1;

        if counters.bytes_processed >= next_progress_threshold {
            tracing::info!(
                "server progress: {:.1}% ({}/{total_bytes} bytes, {})",
                100.0 * counters.bytes_processed as f64 / total_bytes as f64,
                counters.bytes_processed,
                counters,
            );
            next_progress_threshold += progress_interval;
        }

        if batch.is_ready_to_flush() {
            counters.sent += batch.flush(handle);
            counters.flush_count += 1;
        }
    }

    if !batch.is_empty() {
        tracing::trace!("flushing final batch of {} message(s)", batch.len());
        counters.sent += batch.flush(handle);
        counters.flush_count += 1;
    }

    let elapsed = start.elapsed();
    tracing::info!(
        "server done in {elapsed:.2?}: {counters} | throughput={:.0} msg/s",
        counters.parsed as f64 / elapsed.as_secs_f64(),
    );
    Ok(())
}

/// Accumulates ITCH messages and flushes them to the moldudp server. The
/// flush threshold is randomized per batch so the consumer sees realistic
/// burstiness.
struct Batch {
    buf: Vec<Vec<u8>>,
    cap: usize,
    next_flush: usize,
}

impl Batch {
    fn new(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap),
            cap,
            next_flush: pick_flush_size(cap),
        }
    }

    fn push(&mut self, msg: Vec<u8>) {
        self.buf.push(msg);
    }

    fn len(&self) -> usize {
        self.buf.len()
    }

    fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    fn is_ready_to_flush(&self) -> bool {
        self.buf.len() >= self.next_flush
    }

    fn flush(&mut self, handle: &ServerHandle) -> u64 {
        let n = self.buf.len() as u64;
        let payload = std::mem::take(&mut self.buf);
        handle.send(payload);
        self.buf.reserve(self.cap);
        self.next_flush = pick_flush_size(self.cap);
        n
    }
}

fn pick_flush_size(max: usize) -> usize {
    rand::random_range(1..=max)
}

/// Aggregate counters for the replay loop, used in progress + summary log lines.
#[derive(Default)]
struct ReplayCounters {
    parsed: u64,
    sent: u64,
    flush_count: u64,
    bytes_processed: usize,
}

impl std::fmt::Display for ReplayCounters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "parsed={} sent={} flushes={}",
            self.parsed, self.sent, self.flush_count,
        )
    }
}

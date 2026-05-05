use anyhow::Result;
use memmap2::Mmap;
use moldudp::ServerHandle;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub fn replay(
    mmap: Mmap,
    handle: &ServerHandle,
    shutdown: Arc<AtomicBool>,
    max_msgs: usize,
) -> Result<()> {
    log::info!("server thread started");
    let start = Instant::now();

    let mut buf: &[u8] = &mmap;
    let total_bytes = buf.len();
    let mut batch: Vec<Vec<u8>> = Vec::with_capacity(max_msgs);
    let mut next_flush = pick_flush_size(max_msgs);
    let mut parsed: u64 = 0;
    let mut sent: u64 = 0;
    let mut flush_count: u64 = 0;
    let mut bytes_processed: usize = 0;

    // Progress logging: emit a line every ~5 % of file.
    let progress_interval = (total_bytes / 20).max(1);
    let mut next_progress_threshold = progress_interval;

    while !buf.is_empty() {
        if shutdown.load(Ordering::Relaxed) {
            let offset = total_bytes - buf.len();
            log::info!(
                "server shutdown requested at offset {offset}/{total_bytes} \
                 ({:.1}%, parsed={parsed}, sent={sent})",
                100.0 * offset as f64 / total_bytes as f64,
            );
            break;
        }

        let before = buf.len();
        let (msg, rest) = match itch5::parse_one(buf) {
            Ok(t) => t,
            Err(e) => {
                let offset = total_bytes - buf.len();
                log::error!(
                    "parse error at offset {offset} ({} bytes remaining): {e:?}",
                    buf.len()
                );
                break;
            }
        };

        if rest.len() >= before {
            let offset = total_bytes - before;
            log::error!("parser made no progress at offset {offset}; aborting");
            break;
        }

        let msg_bytes = before - rest.len();
        bytes_processed += msg_bytes;
        log::trace!(
            "parsed message at offset={} size={msg_bytes}",
            total_bytes - before
        );

        buf = rest;
        batch.push(msg.to_vec());
        parsed += 1;

        // ── Progress reporting ─────────────────────────────────────────────
        if bytes_processed >= next_progress_threshold {
            log::info!(
                "server progress: {:.1}% ({bytes_processed}/{total_bytes} bytes, \
                 parsed={parsed}, sent={sent}, flushes={flush_count})",
                100.0 * bytes_processed as f64 / total_bytes as f64,
            );
            next_progress_threshold += progress_interval;
        }

        if batch.len() >= next_flush {
            log::trace!(
                "flushing batch of {} message(s) (flush #{})",
                batch.len(),
                flush_count + 1,
            );
            sent += flush(handle, &mut batch, max_msgs)?;
            flush_count += 1;
            next_flush = pick_flush_size(max_msgs);
            log::debug!("flush #{flush_count} done; next flush at {next_flush} messages");
        }
    }

    // Flush remainder.
    if !batch.is_empty() {
        log::trace!("flushing final batch of {} message(s)", batch.len());
        sent += flush(handle, &mut batch, max_msgs)?;
        flush_count += 1;
        log::debug!("final flush done (flush #{flush_count})");
    }

    let elapsed = start.elapsed();
    log::info!(
        "server done in {:.2?}: parsed={parsed} sent={sent} flushes={flush_count} \
         throughput={:.0} msg/s",
        elapsed,
        parsed as f64 / elapsed.as_secs_f64(),
    );
    Ok(())
}

fn pick_flush_size(max: usize) -> usize {
    rand::random_range(1..=max)
}

fn flush(handle: &ServerHandle, batch: &mut Vec<Vec<u8>>, cap: usize) -> Result<u64> {
    let n = batch.len() as u64;
    let payload = std::mem::take(batch);
    handle.send(payload);
    batch.reserve(cap);
    Ok(n)
}

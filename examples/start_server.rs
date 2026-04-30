use anyhow::{Context, Result, bail};
use memmap2::Mmap;
use moldudp::MoldUDP64Server;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const DATA_PATH: &str = "examples/data/itch_1000_000";
const MULTICAST_ADDR: &str = "239.1.2.3:5000";
const REREQUEST_ADDR: &str = "127.0.0.1:6000";
const SESSION: &str = "TESTSESSN";
const MAX_MSGS: usize = 100;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let path = Path::new(DATA_PATH);
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;

    let file_len = file.metadata().context("reading file metadata")?.len();
    if file_len == 0 {
        bail!("file is empty: {}", path.display());
    }

    // SAFETY: assumes the file is not modified or truncated for the lifetime
    // of the mmap.
    let mmap =
        unsafe { Mmap::map(&file) }.with_context(|| format!("mmap'ing {}", path.display()))?;

    let multicast_addr = MULTICAST_ADDR
        .parse()
        .with_context(|| format!("invalid multicast address: {MULTICAST_ADDR}"))?;
    let rerequest_addr = REREQUEST_ADDR
        .parse()
        .with_context(|| format!("invalid rerequest address: {REREQUEST_ADDR}"))?;

    if SESSION.len() > 10 {
        bail!(
            "session must be <= 10 chars (MoldUDP64 wire format), got {}",
            SESSION.len()
        );
    }

    let server = MoldUDP64Server::builder()
        .multicast_addr(multicast_addr)
        .rerequest_bind_addr(rerequest_addr)
        .session(SESSION.to_string())
        .build();

    let handle = server.start().context("starting MoldUDP64 server")?;

    let shutdown = Arc::new(AtomicBool::new(false));
    {
        let s = Arc::clone(&shutdown);
        if let Err(e) = ctrlc::set_handler(move || s.store(true, Ordering::Relaxed)) {
            log::warn!("could not install Ctrl-C handler: {e}");
        }
    }

    let total_bytes = mmap.len();
    let mut buf: &[u8] = &mmap;
    let mut batch: Vec<Vec<u8>> = Vec::with_capacity(MAX_MSGS);
    let mut next_flush = pick_flush_size();
    let mut parsed: u64 = 0;
    let mut sent: u64 = 0;
    while !buf.is_empty() {
        if shutdown.load(Ordering::Relaxed) {
            log::info!("shutdown requested at offset {}", total_bytes - buf.len());
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

        buf = rest;
        batch.push(msg.to_vec());
        parsed += 1;

        if batch.len() >= next_flush {
            sent += flush(&handle, &mut batch)?;
            next_flush = pick_flush_size();
        }
    }

    if !batch.is_empty() {
        sent += flush(&handle, &mut batch)?;
    }

    log::info!("done: parsed {parsed} messages, sent {sent}");
    Ok(())
}

fn pick_flush_size() -> usize {
    rand::random_range(1..=MAX_MSGS)
}

fn flush(handle: &moldudp::ServerHandle, batch: &mut Vec<Vec<u8>>) -> Result<u64> {
    let n = batch.len() as u64;
    let payload = std::mem::take(batch);
    handle.send(payload);
    batch.reserve(MAX_MSGS);
    Ok(n)
}

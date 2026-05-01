use anyhow::{Context, Result, bail};
use clap::Parser;
use memmap2::Mmap;
use moldudp::MoldUDP64Server;
use std::fs::File;
use std::net::{SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const DEFAULT_MULTICAST_ADDR: &str = "239.1.2.3:5000";
const DEFAULT_REREQUEST_ADDR: &str = "127.0.0.1:6000";
const DEFAULT_SESSION: &str = "TESTSESSN";
const DEFAULT_MAX_MSGS: usize = 100;

/// Start a MoldUDP64 server that replays historical ITCH5 data.
#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
struct Config {
    /// Path to itch5 file to replay.
    #[arg(short = 'f', long = "file")]
    file_path: PathBuf,

    /// Session identifier to use. Strings shorter than 10 bytes are
    /// right-padded with spaces; longer strings are truncated.
    #[arg(short = 's', long, default_value = DEFAULT_SESSION)]
    session: String,

    /// Destination for live packets.
    #[arg(short = 'm', long, default_value_t = DEFAULT_MULTICAST_ADDR.parse().unwrap())]
    multicast_addr: SocketAddrV4,

    /// Local bind address for the unicast re-request server. The client
    /// sends RetransmissionRequest datagrams here.
    #[arg(short = 'r', long, default_value_t = DEFAULT_REREQUEST_ADDR.parse().unwrap())]
    rerequest_addr: SocketAddr,

    /// Max number of messages to send per batch.
    /// Note: Depending on total length of batch, the batch may be sent in separate packets due to MTU.
    #[arg(long, default_value_t = DEFAULT_MAX_MSGS)]
    max_msgs: usize,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let config = Config::parse();

    let file = File::open(&config.file_path)
        .with_context(|| format!("opening {}", config.file_path.display()))?;

    let file_len = file.metadata().context("reading file metadata")?.len();
    if file_len == 0 {
        bail!("file is empty: {}", config.file_path.display());
    }

    let mmap = unsafe { Mmap::map(&file) }
        .with_context(|| format!("mmap'ing {}", config.file_path.display()))?;

    let server = MoldUDP64Server::builder()
        .multicast_addr(config.multicast_addr)
        .rerequest_bind_addr(config.rerequest_addr)
        .session(config.session)
        .build();

    let handle = server.start().context("starting MoldUDP64 server")?;

    let shutdown = Arc::new(AtomicBool::new(false));
    {
        let s = Arc::clone(&shutdown);
        if let Err(e) = ctrlc::set_handler(move || s.store(true, Ordering::Relaxed)) {
            log::warn!("could not install Ctrl-C handler: {e}");
        }
    }

    serve(mmap, handle, shutdown, config.max_msgs)
}

fn serve(
    mmap: Mmap,
    handle: moldudp::ServerHandle,
    shutdown: Arc<AtomicBool>,
    max_msgs: usize,
) -> Result<()> {
    let mut buf: &[u8] = &mmap;
    let total_bytes = buf.len();
    let mut batch: Vec<Vec<u8>> = Vec::with_capacity(max_msgs);
    let mut next_flush = pick_flush_size(max_msgs);
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
            next_flush = pick_flush_size(max_msgs);
        }
    }

    if !batch.is_empty() {
        sent += flush(&handle, &mut batch)?;
    }

    log::info!("done: parsed {parsed} messages, sent {sent}");
    Ok(())
}

fn pick_flush_size(max: usize) -> usize {
    rand::random_range(1..=max)
}

fn flush(handle: &moldudp::ServerHandle, batch: &mut Vec<Vec<u8>>) -> Result<u64> {
    let n = batch.len() as u64;
    let payload = std::mem::take(batch);
    handle.send(payload);
    batch.reserve(DEFAULT_MAX_MSGS);
    Ok(n)
}

use anyhow::{Context, Result, bail};
use app::config::Config;
use app::handler::{MessageHandler, start_handler};
use app::replay::replay;
use app::report::write_report;
use clap::Parser;
use itch5::messages::*;
use memmap2::Mmap;
use moldudp::{MoldUDP64, MoldUDP64Server};
use orderbook::registry::VecRegistry;
use std::fs::File;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{io, thread};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let config = Config::parse();
    let symbol_strs: Option<Vec<String>> = config
        .symbol_strs
        .map(|ss| ss.iter().map(|s| s.to_uppercase()).collect());

    // ── Configuration summary ──────────────────────────────────────────────
    log::info!(
        "starting itch5-replay: file={} session={} multicast={} rerequest={} interface={} max_msgs={}",
        config.file_path.display(),
        config.session,
        config.multicast_addr,
        config.rerequest_addr,
        config.interface_addr,
        config.max_msgs,
    );
    if symbol_strs.is_none() {
        log::info!("no symbols specified with --watch, watching all symbols");
    } else {
        log::info!(
            "watching {} symbol(s): {}",
            symbol_strs.as_ref().unwrap().len(),
            symbol_strs.as_ref().unwrap().join(", ")
        );
    }
    if let Some(ref p) = config.output_file_path {
        log::info!("report will be written to {}", p.display());
    } else {
        log::info!("report will be written to stdout");
    }

    // ── File validation ────────────────────────────────────────────────────
    let input_file = File::open(&config.file_path)
        .with_context(|| format!("opening {}", config.file_path.display()))?;

    let file_len = input_file
        .metadata()
        .context("reading file metadata")?
        .len();
    if file_len == 0 {
        bail!("file is empty: {}", config.file_path.display());
    }
    log::info!(
        "input file: {} ({:.2} MiB)",
        config.file_path.display(),
        file_len as f64 / 1024.0 / 1024.0,
    );

    let mmap = unsafe { Mmap::map(&input_file) }
        .with_context(|| format!("mmap'ing {}", config.file_path.display()))?;
    log::debug!("mmap established: {} bytes", mmap.len());

    // ── Server + client startup ────────────────────────────────────────────
    log::debug!("building MoldUDP64 server");
    let server = MoldUDP64Server::builder()
        .multicast_addr(config.multicast_addr)
        .rerequest_bind_addr(config.rerequest_addr)
        .session(config.session.clone())
        .build();
    let server_handle = server.start().context("starting MoldUDP64 server")?;
    log::info!(
        "MoldUDP64 server started (multicast={}, rerequest={})",
        config.multicast_addr,
        config.rerequest_addr
    );

    log::debug!("building MoldUDP64 client");
    let (rx, req_tx) = MoldUDP64::builder()
        .multicast_addr(config.multicast_addr)
        .interface_addr(config.interface_addr)
        .rerequest_server_addrs(vec![config.rerequest_addr])
        .expected_session_ident(config.session.clone())
        .build()
        .start()
        .context("starting MoldUDP64 client")?;
    log::info!(
        "MoldUDP64 client started (joined multicast={} on interface={})",
        config.multicast_addr,
        config.interface_addr
    );

    // ── Ctrl-C handler ─────────────────────────────────────────────────────
    let shutdown = Arc::new(AtomicBool::new(false));
    {
        let s = Arc::clone(&shutdown);
        match ctrlc::set_handler(move || {
            log::info!("Ctrl-C received; requesting shutdown");
            s.store(true, Ordering::Relaxed);
        }) {
            Ok(()) => log::debug!("Ctrl-C handler installed"),
            Err(e) => log::warn!("could not install Ctrl-C handler: {e}"),
        }
    }

    // Give the client time to bind and join the multicast group.
    log::debug!("sleeping 100 ms to let client join multicast group before server streams");
    thread::sleep(Duration::from_millis(100));

    // ── Replay thread ──────────────────────────────────────────────────────
    let server_thread = {
        let shutdown = shutdown.clone();
        let max_msgs = config.max_msgs;
        thread::spawn(move || -> Result<()> {
            log::debug!("server thread spawned");
            let res = replay(mmap, &server_handle, shutdown, max_msgs);
            log::info!("server sending end-of-session marker");
            server_handle.end_of_session();
            log::debug!("server thread exiting");
            res
        })
    };

    // ── Handler thread ──────────────────────────────────────────────────────
    let client_thread = {
        let symbols_to_watch: Option<Vec<Symbol>> = symbol_strs.map(|ss| {
            ss.iter()
                .map(|s| {
                    itch5::messages::Symbol::from_str(s.as_str())
                        .expect("Symbol::from_str only returns err if len == 0")
                })
                .collect()
        });

        thread::spawn(move || -> MessageHandler<VecRegistry> {
            log::info!("client thread started");
            start_handler(symbols_to_watch, rx, req_tx, shutdown)
        })
    };

    // ── Join threads ───────────────────────────────────────────────────────
    log::debug!("waiting for server thread to finish");
    let server_res = server_thread.join().expect("server thread panicked");
    log::debug!("waiting for client thread to finish");
    let handler = client_thread.join().expect("client thread panicked");
    server_res?;

    // ── Write report ───────────────────────────────────────────────────────
    if let Some(path) = config.output_file_path {
        log::info!("writing report to {}", path.display());
        let output_file = File::create(&path)
            .with_context(|| format!("creating output file {}", path.display()))?;
        write_report(&handler, output_file)?;
        log::info!("report written to {}", path.display());
    } else {
        log::debug!("writing report to stdout");
        write_report(&handler, io::stdout())?;
    }

    log::info!("done");
    Ok(())
}

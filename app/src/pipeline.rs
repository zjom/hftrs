//! End-to-end orchestration: opens the input file, brings up the moldudp
//! server + client, spawns the replay/handler threads, and writes the report.
//!
//! This is the single entry point for running the app from `main` *or* from
//! integration tests.

use crate::config::Config;
use crate::handler::{self, MessageHandler, SharedHandler};
use crate::replay::replay;
use crate::report;
use crate::tui;
use anyhow::{Context, Result, bail};
use itch5::messages::Symbol;
use memmap2::Mmap;
use moldudp::{MoldUDP64, MoldUDP64Server};
use orderbook::registry::VecRegistry;
use std::fs::File;
use std::io;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Run the full pipeline as configured. Returns once the file is exhausted
/// (or shutdown is requested) and the report has been written.
pub fn run(config: Config) -> Result<()> {
    log_config_summary(&config);
    let symbols = parse_symbols(config.symbol_strs.as_deref());

    let mmap = open_input(&config.file_path)?;
    let server_handle = start_server(&config)?;
    let (rx, req_tx) = start_client(&config)?;
    let shutdown = install_shutdown_handler();

    // Give the client time to bind and join the multicast group before the
    // server starts streaming.
    tracing::debug!("sleeping 100 ms to let client join multicast group before server streams");
    thread::sleep(Duration::from_millis(100));

    let shared_handler: SharedHandler<VecRegistry> =
        Arc::new(Mutex::new(MessageHandler::<VecRegistry>::new(
            symbols,
            Arc::clone(&shutdown),
        )));

    let server_thread = {
        let shutdown = Arc::clone(&shutdown);
        let max_msgs = config.max_msgs;
        thread::spawn(move || -> Result<()> {
            tracing::debug!("server thread spawned");
            let res = replay(mmap, &server_handle, shutdown, max_msgs);
            tracing::info!("server sending end-of-session marker");
            server_handle.end_of_session();
            tracing::debug!("server thread exiting");
            res
        })
    };

    let client_thread = {
        let shutdown = Arc::clone(&shutdown);
        let handler = Arc::clone(&shared_handler);
        thread::spawn(move || {
            tracing::info!("client thread started");
            handler::run(handler, rx, req_tx, shutdown);
        })
    };

    let tui_thread = if config.interactive {
        let shutdown = Arc::clone(&shutdown);
        let handler = Arc::clone(&shared_handler);
        Some(thread::spawn(move || -> Result<()> {
            tracing::debug!("tui thread spawned");
            tui::run(handler, shutdown)
        }))
    } else {
        None
    };

    tracing::debug!("waiting for server thread to finish");
    let server_res = server_thread.join().expect("server thread panicked");
    tracing::debug!("waiting for client thread to finish");
    client_thread.join().expect("client thread panicked");
    if let Some(t) = tui_thread {
        tracing::debug!("waiting for tui thread to finish");
        t.join().expect("tui thread panicked")?;
    }
    server_res?;

    let handler = Arc::try_unwrap(shared_handler)
        .map_err(|_| anyhow::anyhow!("handler still has outstanding references"))?
        .into_inner()
        .expect("handler mutex poisoned");

    if config.should_make_report() {
        write_report(&handler, &config)?;
    }
    tracing::info!("done");
    Ok(())
}

fn log_config_summary(config: &Config) {
    tracing::info!(
        "starting itch5-replay: file={} session={} multicast={} rerequest={} interface={} \
         max_msgs={} format={} depth={}",
        config.file_path.display(),
        config.session,
        config.multicast_addr,
        config.rerequest_addr,
        config.interface_addr,
        config.max_msgs,
        config.report_format,
        config.report_depth,
    );
    if let Some(ss) = &config.symbol_strs {
        tracing::info!("watching {} symbol(s): {}", ss.len(), ss.join(", "));
    } else {
        tracing::info!("no symbols specified with --watch, watching all symbols");
    }

    if !config.should_make_report() {
        tracing::info!(
            "running in interactive mode with no output file path specified. no report will be produced"
        );
    } else if let Some(p) = &config.output_file_path {
        tracing::info!("report will be written to {}", p.display());
    } else {
        tracing::info!("report will be written to stdout");
    }
}

fn parse_symbols(symbol_strs: Option<&[String]>) -> Option<Vec<Symbol>> {
    symbol_strs.map(|ss| {
        ss.iter()
            .map(|s| {
                Symbol::from_str(&s.to_uppercase())
                    .expect("Symbol::from_str only returns err if len == 0")
            })
            .collect()
    })
}

fn open_input(path: &std::path::Path) -> Result<Mmap> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let len = file.metadata().context("reading file metadata")?.len();
    if len == 0 {
        bail!("file is empty: {}", path.display());
    }
    tracing::info!(
        "input file: {} ({:.2} MiB)",
        path.display(),
        len as f64 / 1024.0 / 1024.0,
    );
    let mmap =
        unsafe { Mmap::map(&file) }.with_context(|| format!("mmap'ing {}", path.display()))?;
    tracing::debug!("mmap established: {} bytes", mmap.len());
    Ok(mmap)
}

fn start_server(config: &Config) -> Result<moldudp::ServerHandle> {
    tracing::debug!("building MoldUDP64 server");
    let server = MoldUDP64Server::builder()
        .multicast_addr(config.multicast_addr)
        .rerequest_bind_addr(config.rerequest_addr)
        .session(config.session.clone())
        .build();
    let handle = server.start().context("starting MoldUDP64 server")?;
    tracing::info!(
        "MoldUDP64 server started (multicast={}, rerequest={})",
        config.multicast_addr,
        config.rerequest_addr,
    );
    Ok(handle)
}

type ClientChannels = (
    moldudp::Receiver<moldudp::Datagram>,
    moldudp::Sender<moldudp::RetransmissionRequest>,
);

fn start_client(config: &Config) -> Result<ClientChannels> {
    tracing::debug!("building MoldUDP64 client");
    let channels = MoldUDP64::builder()
        .multicast_addr(config.multicast_addr)
        .interface_addr(config.interface_addr)
        .rerequest_server_addrs(vec![config.rerequest_addr])
        .expected_session_ident(config.session.clone())
        .build()
        .start()
        .context("starting MoldUDP64 client")?;
    tracing::info!(
        "MoldUDP64 client started (joined multicast={} on interface={})",
        config.multicast_addr,
        config.interface_addr,
    );
    Ok(channels)
}

fn install_shutdown_handler() -> Arc<AtomicBool> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let s = Arc::clone(&shutdown);
    match ctrlc::set_handler(move || {
        tracing::info!("Ctrl-C received; requesting shutdown");
        s.store(true, Ordering::Relaxed);
    }) {
        Ok(()) => tracing::debug!("Ctrl-C handler installed"),
        Err(e) => tracing::warn!("could not install Ctrl-C handler: {e}"),
    }
    shutdown
}

fn write_report(handler: &MessageHandler<VecRegistry>, config: &Config) -> Result<()> {
    if let Some(path) = &config.output_file_path {
        tracing::info!("writing report to {}", path.display());
        let file = File::create(path)
            .with_context(|| format!("creating output file {}", path.display()))?;
        report::write(handler, config.report_format, config.report_depth, file)?;
        tracing::info!("report written to {}", path.display());
    } else {
        tracing::debug!("writing report to stdout");
        report::write(
            handler,
            config.report_format,
            config.report_depth,
            io::stdout(),
        )?;
    }
    Ok(())
}

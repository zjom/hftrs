use anyhow::{Context, Result, bail};
use clap::Parser;
use itch5::messages::*;
use memmap2::Mmap;
use moldudp::{
    FromBytes, MoldUDP64, MoldUDP64Server, Packet, PacketKind, RetransmissionPacket,
    RetransmissionRequest, ServerHandle,
};
use orderbook::registry::{Registry, VecRegistry};
use orderbook::{Order, Side};
use std::collections::HashSet;
use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::{io, thread};

const DEFAULT_MULTICAST_ADDR: &str = "239.1.2.3:5000";
const DEFAULT_REREQUEST_ADDR: &str = "127.0.0.1:6000";
const DEFAULT_SESSION: &str = "TESTSESSN";
const DEFAULT_MAX_MSGS: usize = 100;

/// Replay an ITCH5 file over MoldUDP64: the server thread streams packets to a
/// multicast group while the client thread receives them and builds order books.
#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
struct Config {
    /// Path to itch5 file to replay.
    #[arg(short = 'f', long = "file")]
    file_path: PathBuf,

    /// Path to file to write output.
    #[arg(short = 'o', long = "out")]
    output_file_path: Option<PathBuf>,

    /// Symbols to watch
    #[arg(short = 'w', long = "watch")]
    symbol_strs: Option<Vec<String>>,

    /// Session identifier. Strings shorter than 10 bytes are right-padded with
    /// spaces; longer strings are truncated.
    #[arg(short = 's', long, default_value = DEFAULT_SESSION)]
    session: String,

    /// Multicast group + port shared by the server (sender) and client (receiver).
    #[arg(short = 'm', long, default_value_t = DEFAULT_MULTICAST_ADDR.parse().unwrap())]
    multicast_addr: SocketAddrV4,

    /// Local bind address for the unicast re-request server. The client also
    /// sends RetransmissionRequest datagrams to this address.
    #[arg(short = 'r', long, default_value_t = DEFAULT_REREQUEST_ADDR.parse().unwrap())]
    rerequest_addr: SocketAddr,

    /// Local interface used by the client to join the multicast group.
    #[arg(short = 'i', long, default_value_t = Ipv4Addr::UNSPECIFIED)]
    interface_addr: Ipv4Addr,

    /// Max number of messages to send per batch.
    /// Note: Depending on total length of batch, the batch may be sent in
    /// separate packets due to MTU.
    #[arg(long, default_value_t = DEFAULT_MAX_MSGS)]
    max_msgs: usize,
}

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

    // ── Server thread ──────────────────────────────────────────────────────
    let server_thread = {
        let shutdown = shutdown.clone();
        let max_msgs = config.max_msgs;
        thread::spawn(move || -> Result<()> {
            log::debug!("server thread spawned");
            let res = serve(mmap, &server_handle, shutdown, max_msgs);
            log::info!("server sending end-of-session marker");
            server_handle.end_of_session();
            log::debug!("server thread exiting");
            res
        })
    };

    // ── Client thread ──────────────────────────────────────────────────────
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
            let client_start = Instant::now();

            let mut handler = match symbols_to_watch {
                Some(ss) => MessageHandler::<VecRegistry>::with_symbols(ss),
                None => MessageHandler::new(),
            };

            // Per-run counters.
            let mut packets_received: u64 = 0;
            let mut heartbeats: u64 = 0;
            let mut retransmission_requests: u64 = 0;
            let mut parse_errors: u64 = 0;
            let mut truncated_packets: u64 = 0;

            while let Ok(datagram) = rx.recv() {
                if shutdown.load(Ordering::Relaxed) {
                    log::info!(
                        "client shutdown requested after {} packets ({} heartbeats, {} rereqs)",
                        packets_received,
                        heartbeats,
                        retransmission_requests,
                    );
                    break;
                }

                let packet = Packet::ref_from_bytes(datagram.bytes()).unwrap();
                let msg_count = packet.msg_count();
                let seq_num = packet.seq_num();

                match packet.packet_kind() {
                    PacketKind::Heartbeat => {
                        heartbeats += 1;
                        log::trace!("heartbeat (total={})", heartbeats);
                        continue;
                    }
                    PacketKind::EndOfSession => {
                        log::info!("end-of-session packet received at seq={}", seq_num);
                        break;
                    }
                    _ => {}
                }

                packets_received += 1;
                log::debug!(
                    "packet received: seq={} msg_count={} (total_packets={})",
                    seq_num,
                    msg_count,
                    packets_received,
                );

                // Integrity check: does the payload actually contain the
                // advertised number of messages?
                let actual_msg_count = packet.iter().len();
                if actual_msg_count != msg_count.into() {
                    truncated_packets += 1;
                    log::warn!(
                        "truncated packet at seq={}: advertised {} messages but found {} \
                         (total truncated={}); sending retransmission request",
                        seq_num,
                        msg_count,
                        actual_msg_count,
                        truncated_packets,
                    );
                    let rereq = RetransmissionPacket {
                        msg_count: msg_count.into(),
                        seq_num: seq_num.into(),
                        session: *packet.session_ident_raw(),
                    };
                    match req_tx.try_send(RetransmissionRequest::new(rereq)) {
                        Ok(()) => {
                            retransmission_requests += 1;
                            log::debug!(
                                "retransmission request sent for seq={} (total_rereqs={})",
                                seq_num,
                                retransmission_requests,
                            );
                        }
                        Err(e) => log::error!(
                            "failed to enqueue retransmission request for seq={}: {e}",
                            seq_num
                        ),
                    }
                }

                log::trace!(
                    "parsing {} message(s) from seq={}",
                    actual_msg_count,
                    seq_num
                );
                if let Err(e) = itch5::Parser::new(&packet.messages).parse_stream(&mut handler) {
                    parse_errors += 1;
                    log::error!(
                        "parse error in packet seq={} (total_parse_errors={}): {e}",
                        seq_num,
                        parse_errors,
                    );
                    continue;
                }
            }

            let elapsed = client_start.elapsed();
            log::info!(
                "client thread done in {:.2?}: packets={} heartbeats={} rereqs={} \
                 truncated={} parse_errors={} | orders_added={} executed={} cancelled={} \
                 deleted={} replaced={} registered_symbols={}",
                elapsed,
                packets_received,
                heartbeats,
                retransmission_requests,
                truncated_packets,
                parse_errors,
                handler.stats.orders_added,
                handler.stats.orders_executed,
                handler.stats.orders_cancelled,
                handler.stats.orders_deleted,
                handler.stats.orders_replaced,
                handler.registry.len(),
            );

            handler
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

// ── Server ─────────────────────────────────────────────────────────────────

fn serve(
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
            log::debug!(
                "flushing batch of {} message(s) (flush #{})",
                batch.len(),
                flush_count + 1,
            );
            sent += flush(handle, &mut batch)?;
            flush_count += 1;
            next_flush = pick_flush_size(max_msgs);
            log::debug!("flush #{flush_count} done; next flush at {next_flush} messages");
        }
    }

    // Flush remainder.
    if !batch.is_empty() {
        log::debug!("flushing final batch of {} message(s)", batch.len());
        sent += flush(handle, &mut batch)?;
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

fn flush(handle: &ServerHandle, batch: &mut Vec<Vec<u8>>) -> Result<u64> {
    let n = batch.len() as u64;
    let payload = std::mem::take(batch);
    handle.send(payload);
    batch.reserve(DEFAULT_MAX_MSGS);
    Ok(n)
}

// ── Message handler ────────────────────────────────────────────────────────

/// Running totals for the client session, used in the final summary log line.
#[derive(Default)]
struct HandlerStats {
    orders_added: u64,
    orders_executed: u64,
    orders_cancelled: u64,
    orders_deleted: u64,
    orders_replaced: u64,
    stock_directory_msgs: u64,
    /// Orders that referenced an unregistered locate code (not in watch list).
    skipped_locate: u64,
}

struct MessageHandler<R: Registry> {
    registry: R,
    symbols_to_watch: Option<HashSet<u64>>,
    stats: HandlerStats,
}

impl<R: Registry> MessageHandler<R> {
    fn new() -> MessageHandler<R> {
        log::debug!("creating MessageHandler for all symbols");
        MessageHandler {
            registry: R::new(),
            symbols_to_watch: None,
            stats: HandlerStats::default(),
        }
    }
}

impl MessageHandler<VecRegistry> {
    fn with_symbols(symbols: Vec<Symbol>) -> MessageHandler<VecRegistry> {
        log::debug!("creating MessageHandler for {} symbol(s)", symbols.len());
        MessageHandler {
            registry: VecRegistry::new(),
            symbols_to_watch: Some(symbols.iter().map(|s| s.to_u64()).collect()),
            stats: HandlerStats::default(),
        }
    }
}

// impl MessageHandler<HashMapRegistry> {
//     fn with_symbols(symbols: Vec<Symbol>) -> MessageHandler<HashMapRegistry> {
//         log::debug!("creating MessageHandler for {} symbol(s)", symbols.len());
//         MessageHandler {
//             registry: HashMapRegistry::with_capacity(symbols.len()),
//             symbols_to_watch: Some(symbols.iter().map(|s| s.to_u64()).collect()),
//             stats: HandlerStats::default(),
//         }
//     }
// }

impl<R: Registry> itch5::MessageHandler for MessageHandler<R> {
    fn on_stock_directory(&mut self, msg: &StockDirectory) -> ControlFlow<()> {
        self.stats.stock_directory_msgs += 1;
        let stock = msg.stock();
        if self
            .symbols_to_watch
            .as_ref()
            .map_or(true, |r| r.contains(&stock.to_u64()))
        {
            log::info!(
                "registering symbol {} with locate={} (total_registered={})",
                stock.as_str(),
                msg.stock_locate(),
                self.registry.len() + 1,
            );
            self.registry.register(msg.stock_locate(), stock);
        } else {
            log::trace!(
                "ignoring stock-directory entry for {} (not in watch list)",
                stock.as_str(),
            );
        }
        ControlFlow::Continue(())
    }

    fn on_add_order_no_mpid_attribution(
        &mut self,
        msg: &AddOrderNoMPIDAttribution,
    ) -> ControlFlow<()> {
        let locate = msg.stock_locate();
        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            log::trace!("add_order (no-mpid): skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        let order_ref = msg.order_reference_number();
        let side = match msg.buy_sell_indicator() {
            BuySellIndicator::Buy => Side::Bid,
            BuySellIndicator::Sell => Side::Ask,
            BuySellIndicator::Unknown(unknown) => {
                log::error!(
                    "add_order (no-mpid): unknown buy/sell indicator '{unknown}' \
                     for order_ref={order_ref} locate={locate}"
                );
                return ControlFlow::Continue(());
            }
        };

        log::debug!(
            "add_order (no-mpid): order_ref={order_ref} side={side:?} \
             price={} qty={} ts={} locate={locate}",
            msg.price().into_i64(),
            msg.shares(),
            msg.timestamp().to_naive_time(),
        );

        match book.add(Order {
            id: order_ref,
            side,
            price: msg.price().into_i64(),
            qty: msg.shares() as u64,
            ts: msg.timestamp().to_u64(),
        }) {
            Ok(()) => self.stats.orders_added += 1,
            Err(e) => log::error!(
                "add_order (no-mpid): failed to add order_ref={order_ref} \
                 locate={locate}: {e}"
            ),
        }
        ControlFlow::Continue(())
    }

    fn on_add_order_with_mpid_attribution(
        &mut self,
        msg: &AddOrderWithMPIDAttribution,
    ) -> ControlFlow<()> {
        let locate = msg.stock_locate();
        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            log::trace!("add_order (mpid): skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        let order_ref = msg.order_reference_number();
        let side = match msg.buy_sell_indicator() {
            BuySellIndicator::Buy => Side::Bid,
            BuySellIndicator::Sell => Side::Ask,
            BuySellIndicator::Unknown(unknown) => {
                log::error!(
                    "add_order (mpid): unknown buy/sell indicator '{unknown}' \
                     for order_ref={order_ref} locate={locate}"
                );
                return ControlFlow::Continue(());
            }
        };

        log::debug!(
            "add_order (mpid): order_ref={order_ref} side={side:?} \
             price={} qty={} ts={} locate={locate}",
            msg.price().into_i64(),
            msg.shares(),
            msg.timestamp().to_naive_time(),
        );

        match book.add(Order {
            id: order_ref,
            side,
            price: msg.price().into_i64(),
            qty: msg.shares() as u64,
            ts: msg.timestamp().to_u64(),
        }) {
            Ok(()) => self.stats.orders_added += 1,
            Err(e) => log::error!(
                "add_order (mpid): failed to add order_ref={order_ref} \
                 locate={locate}: {e}"
            ),
        }
        ControlFlow::Continue(())
    }

    fn on_order_executed(&mut self, msg: &OrderExecuted) -> ControlFlow<()> {
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();

        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            log::trace!("order_executed: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        log::debug!(
            "order_executed: order_ref={order_ref} shares={} ts={} locate={locate}",
            msg.executed_shares(),
            msg.timestamp().to_naive_time(),
        );

        match book.execute(
            order_ref,
            msg.executed_shares() as u64,
            msg.timestamp().to_u64(),
        ) {
            Ok(_) => {
                self.stats.orders_executed += 1;
            }
            Err(e) => {
                log::error!("order_executed: failed for order_ref={order_ref} locate={locate}: {e}")
            }
        }
        ControlFlow::Continue(())
    }

    fn on_order_executed_with_price(&mut self, msg: &OrderExecutedWithPrice) -> ControlFlow<()> {
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();

        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            log::trace!("order_executed_with_price: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        log::debug!(
            "order_executed_with_price: order_ref={order_ref} shares={} price={} ts={} locate={locate}",
            msg.executed_shares(),
            msg.execution_price().into_i64(),
            msg.timestamp().to_naive_time(),
        );

        match book.execute_at(
            order_ref,
            msg.executed_shares() as u64,
            msg.execution_price().into_i64(),
            msg.timestamp().to_u64(),
        ) {
            Ok(_) => {
                self.stats.orders_executed += 1;
            }
            Err(e) => log::error!(
                "order_executed_with_price: failed for order_ref={order_ref} \
                 locate={locate}: {e}"
            ),
        }
        ControlFlow::Continue(())
    }

    fn on_order_cancel(&mut self, msg: &OrderCancel) -> ControlFlow<()> {
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();

        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            log::trace!("order_cancel: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        log::debug!(
            "order_cancel: order_ref={order_ref} cancelled_shares={} locate={locate}",
            msg.cancelled_shares(),
        );

        match book.cancel(order_ref, msg.cancelled_shares() as u64) {
            Ok(()) => self.stats.orders_cancelled += 1,
            Err(e) => {
                log::error!("order_cancel: failed for order_ref={order_ref} locate={locate}: {e}")
            }
        }
        ControlFlow::Continue(())
    }

    fn on_order_delete(&mut self, msg: &OrderDelete) -> ControlFlow<()> {
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();

        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            log::trace!("order_delete: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        log::debug!("order_delete: order_ref={order_ref} locate={locate}");

        match book.delete(order_ref) {
            Ok(()) => self.stats.orders_deleted += 1,
            Err(e) => {
                log::error!("order_delete: failed for order_ref={order_ref} locate={locate}: {e}")
            }
        }
        ControlFlow::Continue(())
    }

    fn on_order_replace(&mut self, msg: &OrderReplace) -> ControlFlow<()> {
        let og_order_ref = msg.original_order_reference_number();
        let locate = msg.stock_locate();
        let new_order_ref = msg.new_order_reference_number();

        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            log::trace!(
                "order_replace: skipping untracked locate={locate} \
                 (og={og_order_ref} new={new_order_ref})"
            );
            return ControlFlow::Continue(());
        };

        log::debug!(
            "order_replace: og={og_order_ref} new={new_order_ref} \
             price={} shares={} ts={} locate={locate}",
            msg.price().into_i64(),
            msg.shares(),
            msg.timestamp().to_naive_time(),
        );

        match book.replace(
            og_order_ref,
            new_order_ref,
            msg.price().into_i64(),
            msg.shares() as u64,
            msg.timestamp().to_u64(),
        ) {
            Ok(()) => self.stats.orders_replaced += 1,
            Err(e) => log::error!(
                "order_replace: failed to replace og={og_order_ref} with \
                 new={new_order_ref} locate={locate}: {e}"
            ),
        }

        ControlFlow::Continue(())
    }
}

// ── Report ─────────────────────────────────────────────────────────────────

fn write_report<R: Registry>(
    handler: &MessageHandler<R>,
    mut writer: impl io::Write,
) -> io::Result<()> {
    let book_count = handler.registry.iter().count();
    log::info!(
        "writing report: {} book(s) | stats: stock_dirs={} added={} executed={} \
         cancelled={} deleted={} replaced={} skipped_locate={}",
        book_count,
        handler.stats.stock_directory_msgs,
        handler.stats.orders_added,
        handler.stats.orders_executed,
        handler.stats.orders_cancelled,
        handler.stats.orders_deleted,
        handler.stats.orders_replaced,
        handler.stats.skipped_locate,
    );

    writeln!(writer, "symb\tbest_ask\tbest_bid\tspread\tmid\tdepth(10)")?;
    handler
        .registry
        .iter()
        .map(|(_locate, symb, book)| {
            let best_ask = book.best_ask().unwrap_or((0, 0));
            let best_bid = book.best_bid().unwrap_or((0, 0));
            let spread = book.spread().unwrap_or(0);
            let mid = book.mid().unwrap_or(0);
            let depth = book.depth(10);

            log::debug!(
                "book {}: best_ask={:?} best_bid={:?} spread={} mid={} depth(10)={:?}",
                symb.as_str(),
                best_ask,
                best_bid,
                spread,
                mid,
                depth,
            );

            writeln!(
                writer,
                "{}\t{:?}\t{:?}\t{:?}\t{:?}\t{:?}",
                symb.as_str(),
                best_ask,
                best_bid,
                spread,
                mid,
                depth,
            )
        })
        .collect::<io::Result<Vec<_>>>()?;

    log::info!("report complete");
    Ok(())
}

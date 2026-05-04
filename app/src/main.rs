use anyhow::{Context, Result, bail};
use clap::Parser;
use itch5::messages::*;
use log::{error, info};
use memmap2::Mmap;
use moldudp::{
    FromBytes, MoldUDP64, MoldUDP64Server, Packet, PacketKind, RetransmissionPacket,
    RetransmissionRequest, ServerHandle,
};
use orderbook::{Order, OrderBook, Side};
use std::fs::File;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
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

    let input_file = File::open(&config.file_path)
        .with_context(|| format!("opening {}", config.file_path.display()))?;

    let file_len = input_file
        .metadata()
        .context("reading file metadata")?
        .len();
    if file_len == 0 {
        bail!("file is empty: {}", config.file_path.display());
    }

    let mmap = unsafe { Mmap::map(&input_file) }
        .with_context(|| format!("mmap'ing {}", config.file_path.display()))?;

    let server = MoldUDP64Server::builder()
        .multicast_addr(config.multicast_addr)
        .rerequest_bind_addr(config.rerequest_addr)
        .session(config.session.clone())
        .build();
    let server_handle = server.start().context("starting MoldUDP64 server")?;

    let (rx, req_tx) = MoldUDP64::builder()
        .multicast_addr(config.multicast_addr)
        .interface_addr(config.interface_addr)
        .rerequest_server_addrs(vec![config.rerequest_addr])
        .expected_session_ident(config.session.clone())
        .build()
        .start()
        .context("starting MoldUDP64 client")?;

    let shutdown = Arc::new(AtomicBool::new(false));
    {
        let s = Arc::clone(&shutdown);
        if let Err(e) = ctrlc::set_handler(move || s.store(true, Ordering::Relaxed)) {
            log::warn!("could not install Ctrl-C handler: {e}");
        }
    }

    // Let the client bind & join the multicast group before the server starts
    // streaming, so we don't miss the very first packets.
    thread::sleep(Duration::from_millis(100));

    let server_thread = {
        let shutdown = shutdown.clone();
        let max_msgs = config.max_msgs;
        thread::spawn(move || -> Result<()> {
            let res = serve(mmap, &server_handle, shutdown, max_msgs);
            server_handle.end_of_session();
            res
        })
    };

    let client_thread = thread::spawn(move || -> MessageHandler {
        let mut handler = MessageHandler::new();
        while let Ok(datagram) = rx.recv() {
            if shutdown.load(Ordering::Relaxed) {
                log::info!("shutdown requested");
                break;
            }

            let packet = Packet::ref_from_bytes(datagram.bytes()).unwrap();
            let msg_count = packet.msg_count();
            match packet.packet_kind() {
                PacketKind::Heartbeat => continue,
                PacketKind::EndOfSession => break,
                _ => {}
            }
            info!("received packet with {} messages", msg_count);

            let actual_msg_count = packet.iter().len();
            if actual_msg_count != msg_count.into() {
                let rereq = RetransmissionPacket {
                    msg_count: msg_count.into(),
                    seq_num: packet.seq_num().into(),
                    session: *packet.session_ident_raw(),
                };
                info!("received: {actual_msg_count}, expected: {msg_count}. rerequesting..");

                req_tx.try_send(RetransmissionRequest::new(rereq)).unwrap();
            }

            if let Err(e) = itch5::Parser::new(&packet.messages).parse_stream(&mut handler) {
                error!("failed to parse msg in stream: {e}");
                continue;
            }
        }
        handler
    });

    let server_res = server_thread.join().expect("server thread panicked");
    let handler = client_thread.join().expect("client thread panicked");
    server_res?;

    if let Some(path) = config.output_file_path {
        let output_file = File::create(path)?;
        write_report(&handler, output_file)?;
    } else {
        write_report(&handler, io::stdout())?;
    }

    Ok(())
}

fn serve(
    mmap: Mmap,
    handle: &ServerHandle,
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
            sent += flush(handle, &mut batch)?;
            next_flush = pick_flush_size(max_msgs);
        }
    }

    if !batch.is_empty() {
        sent += flush(handle, &mut batch)?;
    }

    log::info!("done: parsed {parsed} messages, sent {sent}");
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

const NO_SYMBOL: [u8; 8] = [0; 8];

struct MessageHandler {
    books: Vec<OrderBook>,
    symbols: Vec<[u8; 8]>,
}

impl MessageHandler {
    fn new() -> MessageHandler {
        // Stock directory messages will grow these to the locate range
        // actually used in the session (typically ~10k entries).
        MessageHandler {
            books: Vec::with_capacity(u16::MAX as usize + 1),
            symbols: Vec::with_capacity(u16::MAX as usize + 1),
        }
    }

    /// Grow `books` and `symbols` so `locate` is a valid index. Idempotent.
    /// In a well-formed ITCH session, stock directory messages arrive before
    /// any order activity, so this only actually grows on those messages —
    /// but keeping it on the add path too is cheap defensive insurance.
    #[inline]
    fn ensure_locate(&mut self, locate: u16) {
        let needed = locate as usize + 1;
        if self.books.len() < needed {
            self.books
                .resize_with(needed, || OrderBook::with_capacity(1024));
            self.symbols.resize(needed, NO_SYMBOL);
        }
    }

    /// Resolve a locate back to its ASCII symbol for logging.
    fn symbol_str(&self, locate: u16) -> &str {
        let bytes = &self.symbols[locate as usize];
        std::str::from_utf8(bytes).unwrap_or("?").trim_end()
    }
}

impl itch5::MessageHandler for MessageHandler {
    fn on_stock_directory(&mut self, msg: &StockDirectory) -> ControlFlow<()> {
        let locate = msg.stock_locate();
        self.ensure_locate(locate);
        self.symbols[locate as usize] = *msg.stock();
        ControlFlow::Continue(())
    }

    fn on_add_order_no_mpid_attribution(
        &mut self,
        msg: &AddOrderNoMPIDAttribution,
    ) -> ControlFlow<()> {
        let locate = msg.stock_locate();
        self.ensure_locate(locate);
        let order_ref = msg.order_reference_number();

        let side = match msg.buy_sell_indicator() {
            BuySellIndicator::Buy => Side::Bid,
            BuySellIndicator::Sell => Side::Ask,
            BuySellIndicator::Unknown(unknown) => {
                error!("unknown buy/sell indicator in itch msg: {unknown}");
                return ControlFlow::Continue(());
            }
        };

        if let Err(e) = self.books[locate as usize].add(Order {
            id: order_ref,
            side,
            price: msg.price().into_i64(),
            qty: msg.shares() as u64,
            ts: msg.timestamp(),
        }) {
            error!("failed to add order {order_ref}: {e}");
        }
        ControlFlow::Continue(())
    }

    fn on_add_order_with_mpid_attribution(
        &mut self,
        msg: &AddOrderWithMPIDAttribution,
    ) -> ControlFlow<()> {
        let locate = msg.stock_locate();
        self.ensure_locate(locate);
        let order_ref = msg.order_reference_number();

        let side = match msg.buy_sell_indicator() {
            BuySellIndicator::Buy => Side::Bid,
            BuySellIndicator::Sell => Side::Ask,
            BuySellIndicator::Unknown(unknown) => {
                error!("unknown buy/sell indicator in itch msg: {unknown}");
                return ControlFlow::Continue(());
            }
        };

        if let Err(e) = self.books[locate as usize].add(Order {
            id: order_ref,
            side,
            price: msg.price().into_i64(),
            qty: msg.shares() as u64,
            ts: msg.timestamp(),
        }) {
            error!("failed to add order {order_ref}: {e}");
        }
        ControlFlow::Continue(())
    }

    fn on_order_executed_message(&mut self, msg: &OrderExecutedMessage) -> ControlFlow<()> {
        let order_ref = msg.order_reference_number();

        let locate = msg.stock_locate();

        if let Err(e) = self.books[locate as usize].execute(
            order_ref,
            msg.executed_shares() as u64,
            msg.timestamp(),
        ) {
            error!("failed to execute order {order_ref}: {e}");
        }
        ControlFlow::Continue(())
    }

    fn on_order_executed_with_price_message(
        &mut self,
        msg: &OrderExecutedWithPriceMessage,
    ) -> ControlFlow<()> {
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();

        if let Err(e) = self.books[locate as usize].execute_at(
            order_ref,
            msg.executed_shares() as u64,
            msg.execution_price().into_i64(),
            msg.timestamp(),
        ) {
            error!("failed to execute order {order_ref}: {e}");
        }
        ControlFlow::Continue(())
    }

    fn on_order_cancel_message(&mut self, msg: &OrderCancelMessage) -> ControlFlow<()> {
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();

        if let Err(e) = self.books[locate as usize].cancel(order_ref, msg.cancelled_shares() as u64)
        {
            error!("failed to cancel order {order_ref}: {e}");
        }
        ControlFlow::Continue(())
    }

    fn on_order_delete_message(&mut self, msg: &OrderDeleteMessage) -> ControlFlow<()> {
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();
        if let Err(e) = self.books[locate as usize].delete(order_ref) {
            error!("failed to delete order {order_ref}: {e}");
        }
        ControlFlow::Continue(())
    }

    fn on_order_replace_message(&mut self, msg: &OrderReplaceMessage) -> ControlFlow<()> {
        let og_order_ref = msg.original_order_reference_number();
        let locate = msg.stock_locate();
        let new_order_ref = msg.new_order_reference_number();

        if let Err(e) = self.books[locate as usize].replace(
            og_order_ref,
            new_order_ref,
            msg.price().into_i64(),
            msg.shares() as u64,
            msg.timestamp(),
        ) {
            error!("failed to replace order {og_order_ref} with {new_order_ref}: {e}");
        }
        ControlFlow::Continue(())
    }
}

fn write_report(handler: &MessageHandler, mut writer: impl io::Write) -> io::Result<()> {
    writeln!(writer, "symb\tbest_ask\tbest_bid\tspread\tmid\tdepth(10)")?;
    handler
        .books
        .iter()
        .zip(0..)
        .map(|(book, i)| {
            let symb = handler.symbols[i];
            writeln!(
                writer,
                "{}\t{:?}\t{:?}\t{:?}\t{:?}\t{:?}",
                String::from_utf8(symb.to_vec()).unwrap_or("unknown".to_string()),
                book.best_ask().unwrap_or((0, 0)),
                book.best_bid().unwrap_or((0, 0)),
                book.spread().unwrap_or(0),
                book.mid().unwrap_or(0),
                book.depth(10)
            )
        })
        .collect::<io::Result<Vec<_>>>()?; // collects Ok(())s, short-circuits on first Err
    Ok(())
}

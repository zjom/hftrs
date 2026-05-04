//! End-to-end pipeline benchmark.
//!
//! Wires the full system together — server thread (MoldUDP64) → client
//! thread (MoldUDP64) → ITCH parser → registry of order books — and times
//! the whole thing against a recorded ITCH 5.0 sample. This is the number
//! that approximates "how fast can this stack actually consume a feed".
//!
//! ## Topology
//!
//! ```text
//!     ┌──────────────┐    UDP     ┌──────────────┐
//!     │ MoldUDP64    │ ─────────▶ │ MoldUDP64    │
//!     │ test server  │  loopback  │ client       │
//!     └──────────────┘            └──────────────┘
//!         ▲                              │
//!  pre-framed ITCH                packet.messages
//!         │                              ▼
//!     replayed once         itch5::Parser::parse_stream
//!     per iteration                      │
//!                                        ▼
//!                              Registry<symbol → OrderBook>
//! ```
//!
//! ## Methodology
//!
//! - The sample file is mmap'd once and pre-faulted, then walked once
//!   with `itch5::parse_one` to extract message bodies into a
//!   `Vec<Vec<u8>>` (so wire-format parsing is *not* counted in the
//!   benchmark — we want to time the running system, not the setup).
//! - Bodies are pre-batched into ~MTU-sized chunks, mirroring how a real
//!   server would frame them.
//! - Per iteration, a fresh sender thread streams all chunks while the
//!   bench thread drains datagrams, dispatches ITCH messages into a
//!   fresh registry, and stops once `total_msgs` have been processed.
//! - Throughput is reported in elements (ITCH messages) and bytes.
//!
//! ## Caveats
//!
//! Loopback unicast through `start_with_sockets` exercises the same code
//! paths as a real multicast deployment up to the kernel's UDP layer, but
//! does not capture NIC offload, multicast group membership, or wire
//! contention. The number is a useful upper bound for a single consumer
//! against a single feed.
//!
//! ## Running
//!
//! ```sh
//! taskset -c 3 cargo bench -p app --bench end_to_end
//!
//! # cap the number of messages replayed (default: all of them):
//! END_TO_END_MSGS=200000 cargo bench -p app --bench end_to_end
//!
//! # use a different sample file:
//! ITCH5_BENCH_FILE=/path/to/itch.bin cargo bench -p app --bench end_to_end
//! ```

use std::fs::File;
use std::hint::black_box;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use crossbeam::channel::Receiver;
use memmap2::Mmap;

use itch5::messages::*;
use moldudp::{Datagram, FromBytes, MoldUDP64, MoldUDP64Server, Packet, PacketKind, ServerHandle};
use orderbook::registry::Registry;
use orderbook::{Order, Side};

const SESSION: &str = "BENCHSESHN";
/// MoldUDP packet payload budget — 30 × 38 B + framing fits comfortably
/// in a 1500 B Ethernet MTU. Matches what a production server would emit.
const MSGS_PER_PACKET: usize = 30;

// ─── Fixture loading ───────────────────────────────────────────────────────

fn try_load_sample() -> Option<Mmap> {
    let path: PathBuf = std::env::var_os("ITCH5_BENCH_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../data/itch_1000_000"));
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "skip: ITCH5 sample at {} unavailable ({e}). Set ITCH5_BENCH_FILE to override.",
                path.display()
            );
            return None;
        }
    };
    let mmap = unsafe { Mmap::map(&file).ok()? };
    black_box(mmap.iter().fold(0u8, |a, b| a ^ b));
    Some(mmap)
}

/// Extract ITCH message bodies from `mmap`, optionally capped at `limit`.
/// Each returned `Vec<u8>` is one message body (without the 2-byte ITCH
/// length prefix), suitable for re-framing inside a MoldUDP64 packet.
fn extract_messages(mmap: &[u8], limit: Option<usize>) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut rest: &[u8] = mmap;
    while !rest.is_empty() {
        match itch5::parse_one(rest) {
            Ok((body, next)) => {
                out.push(body.to_vec());
                rest = next;
                if let Some(cap) = limit {
                    if out.len() >= cap {
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
    out
}

fn batch_messages(msgs: Vec<Vec<u8>>, msgs_per_packet: usize) -> Vec<Vec<Vec<u8>>> {
    let mut batches: Vec<Vec<Vec<u8>>> =
        Vec::with_capacity(msgs.len().div_ceil(msgs_per_packet));
    let mut chunk = Vec::with_capacity(msgs_per_packet);
    for m in msgs {
        chunk.push(m);
        if chunk.len() >= msgs_per_packet {
            batches.push(std::mem::take(&mut chunk));
            chunk = Vec::with_capacity(msgs_per_packet);
        }
    }
    if !chunk.is_empty() {
        batches.push(chunk);
    }
    batches
}

// ─── Loopback wiring ───────────────────────────────────────────────────────

fn loopback_pair() -> (Receiver<Datagram>, ServerHandle) {
    let client_downstream = UdpSocket::bind("127.0.0.1:0").unwrap();
    let client_addr = client_downstream.local_addr().unwrap();

    let rereq_server_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let rereq_server_addr = rereq_server_sock.local_addr().unwrap();

    let server_downstream = UdpSocket::bind("127.0.0.1:0").unwrap();

    let server = MoldUDP64Server::builder()
        .multicast_addr(SocketAddrV4::new(
            client_addr.ip().to_string().parse().unwrap(),
            client_addr.port(),
        ))
        .rerequest_bind_addr(rereq_server_addr)
        .session(SESSION.to_string())
        .heartbeat_interval(Duration::from_secs(60))
        .build();

    let handle = server
        .start_with_sockets(server_downstream, rereq_server_sock)
        .unwrap();

    let client_rereq = UdpSocket::bind("127.0.0.1:0").unwrap();

    let client = MoldUDP64::builder()
        .multicast_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        .interface_addr(Ipv4Addr::UNSPECIFIED)
        .rerequest_server_addrs(vec![rereq_server_addr])
        .expected_session_ident(SESSION.to_string())
        .build();

    let (rx, _req_tx) = client
        .start_with_sockets(client_downstream, client_rereq, &[rereq_server_addr])
        .unwrap();

    (rx, handle)
}

// ─── Replay handler ────────────────────────────────────────────────────────

struct ReplayHandler {
    registry: Registry,
    adds: u64,
    execs: u64,
    cancels: u64,
    deletes: u64,
    replaces: u64,
    skipped: u64,
    errors: u64,
}

impl ReplayHandler {
    fn new() -> Self {
        Self {
            registry: Registry::with_capacity(1 << 13),
            adds: 0,
            execs: 0,
            cancels: 0,
            deletes: 0,
            replaces: 0,
            skipped: 0,
            errors: 0,
        }
    }
}

impl itch5::MessageHandler for ReplayHandler {
    fn on_stock_directory(&mut self, msg: &StockDirectory) -> ControlFlow<()> {
        self.registry.register(msg.stock_locate(), msg.stock());
        ControlFlow::Continue(())
    }
    fn on_add_order_no_mpid_attribution(
        &mut self,
        msg: &AddOrderNoMPIDAttribution,
    ) -> ControlFlow<()> {
        let Some(book) = self.registry.get_mut(msg.stock_locate()) else {
            self.skipped += 1;
            return ControlFlow::Continue(());
        };
        let side = match msg.buy_sell_indicator() {
            BuySellIndicator::Buy => Side::Bid,
            BuySellIndicator::Sell => Side::Ask,
            BuySellIndicator::Unknown(_) => return ControlFlow::Continue(()),
        };
        if book
            .add(Order {
                id: msg.order_reference_number(),
                side,
                price: msg.price().into_i64(),
                qty: msg.shares() as u64,
                ts: msg.timestamp().to_u64(),
            })
            .is_ok()
        {
            self.adds += 1;
        } else {
            self.errors += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_add_order_with_mpid_attribution(
        &mut self,
        msg: &AddOrderWithMPIDAttribution,
    ) -> ControlFlow<()> {
        let Some(book) = self.registry.get_mut(msg.stock_locate()) else {
            self.skipped += 1;
            return ControlFlow::Continue(());
        };
        let side = match msg.buy_sell_indicator() {
            BuySellIndicator::Buy => Side::Bid,
            BuySellIndicator::Sell => Side::Ask,
            BuySellIndicator::Unknown(_) => return ControlFlow::Continue(()),
        };
        if book
            .add(Order {
                id: msg.order_reference_number(),
                side,
                price: msg.price().into_i64(),
                qty: msg.shares() as u64,
                ts: msg.timestamp().to_u64(),
            })
            .is_ok()
        {
            self.adds += 1;
        } else {
            self.errors += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_order_executed(&mut self, msg: &OrderExecuted) -> ControlFlow<()> {
        if let Some(book) = self.registry.get_mut(msg.stock_locate()) {
            if book
                .execute(
                    msg.order_reference_number(),
                    msg.executed_shares() as u64,
                    msg.timestamp().to_u64(),
                )
                .is_ok()
            {
                self.execs += 1;
            } else {
                self.errors += 1;
            }
        } else {
            self.skipped += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_order_executed_with_price(&mut self, msg: &OrderExecutedWithPrice) -> ControlFlow<()> {
        if let Some(book) = self.registry.get_mut(msg.stock_locate()) {
            if book
                .execute_at(
                    msg.order_reference_number(),
                    msg.executed_shares() as u64,
                    msg.execution_price().into_i64(),
                    msg.timestamp().to_u64(),
                )
                .is_ok()
            {
                self.execs += 1;
            } else {
                self.errors += 1;
            }
        } else {
            self.skipped += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_order_cancel(&mut self, msg: &OrderCancel) -> ControlFlow<()> {
        if let Some(book) = self.registry.get_mut(msg.stock_locate()) {
            if book
                .cancel(msg.order_reference_number(), msg.cancelled_shares() as u64)
                .is_ok()
            {
                self.cancels += 1;
            } else {
                self.errors += 1;
            }
        } else {
            self.skipped += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_order_delete(&mut self, msg: &OrderDelete) -> ControlFlow<()> {
        if let Some(book) = self.registry.get_mut(msg.stock_locate()) {
            if book.delete(msg.order_reference_number()).is_ok() {
                self.deletes += 1;
            } else {
                self.errors += 1;
            }
        } else {
            self.skipped += 1;
        }
        ControlFlow::Continue(())
    }
    fn on_order_replace(&mut self, msg: &OrderReplace) -> ControlFlow<()> {
        if let Some(book) = self.registry.get_mut(msg.stock_locate()) {
            if book
                .replace(
                    msg.original_order_reference_number(),
                    msg.new_order_reference_number(),
                    msg.price().into_i64(),
                    msg.shares() as u64,
                    msg.timestamp().to_u64(),
                )
                .is_ok()
            {
                self.replaces += 1;
            } else {
                self.errors += 1;
            }
        } else {
            self.skipped += 1;
        }
        ControlFlow::Continue(())
    }
}

// ─── Bench ─────────────────────────────────────────────────────────────────

fn bench_end_to_end(c: &mut Criterion) {
    let Some(mmap) = try_load_sample() else { return };

    let limit: Option<usize> = std::env::var("END_TO_END_MSGS")
        .ok()
        .and_then(|s| s.parse().ok());

    let messages = extract_messages(&mmap, limit);
    let total_msgs = messages.len();
    let total_bytes: u64 = messages.iter().map(|m| m.len() as u64).sum();
    if total_msgs == 0 {
        eprintln!("skip: extracted 0 messages from sample");
        return;
    }
    let batches = batch_messages(messages, MSGS_PER_PACKET);

    eprintln!(
        "end_to_end: replaying {total_msgs} ITCH messages across {} packets",
        batches.len()
    );

    let (rx, handle) = loopback_pair();
    // Let the client multicast loop reach its first `recv` before sending.
    std::thread::sleep(Duration::from_millis(50));

    let mut g = c.benchmark_group("app/end_to_end");
    g.throughput(Throughput::ElementsAndBytes {
        elements: total_msgs as u64,
        bytes: total_bytes,
    });
    g.sample_size(10);
    g.measurement_time(Duration::from_secs(20));

    // Chunk size kept well under MoldUDP's internal data channel capacity
    // (1024 datagrams) so the sender can't outrun the user-side reader and
    // start dropping live packets before gap detection kicks in. Send a
    // chunk, drain its messages, then send the next — sustained pipeline,
    // not burst.
    const CHUNK_PACKETS: usize = 512;

    g.bench_function("server→client→parser→book", |b| {
        b.iter(|| {
            let mut handler = ReplayHandler::new();
            let mut delivered: usize = 0;

            for chunk in batches.chunks(CHUNK_PACKETS) {
                let chunk_msgs: usize = chunk.iter().map(|b| b.len()).sum();
                for batch in chunk {
                    handle.send(batch.clone());
                }
                let target = delivered + chunk_msgs;
                while delivered < target {
                    let dgram = match rx.recv_timeout(Duration::from_secs(10)) {
                        Ok(d) => d,
                        Err(_) => panic!(
                            "end_to_end stalled at {delivered}/{total_msgs} messages"
                        ),
                    };
                    let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
                    match pkt.packet_kind() {
                        PacketKind::Heartbeat | PacketKind::EndOfSession => continue,
                        _ => {}
                    }
                    delivered += pkt.msg_count() as usize;
                    itch5::Parser::new(&pkt.messages)
                        .parse_stream(&mut handler)
                        .unwrap();
                }
            }
            black_box((
                handler.adds,
                handler.execs,
                handler.cancels,
                handler.deletes,
                handler.replaces,
                handler.skipped,
                handler.errors,
            ));
        });
    });

    g.finish();
}

criterion_group!(benches, bench_end_to_end);
criterion_main!(benches);

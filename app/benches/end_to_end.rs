//! End-to-end pipeline benchmark.
//!
//! Wires the consumer half of the system together — MoldUDP64 client →
//! ITCH parser → registry of order books — and times the whole thing
//! against a recorded ITCH 5.0 sample. The MoldUDP64 server is *not*
//! benchmarked; pre-framed packets are pumped at the client's downstream
//! socket from a dedicated sender thread so the bench thread does only
//! receive + parse + dispatch work.
//!
//! ## Topology
//!
//! ```text
//!     ┌──────────────┐    UDP     ┌──────────────┐
//!     │ sender       │ ─────────▶ │ MoldUDP64    │
//!     │ thread       │  loopback  │ client       │
//!     └──────────────┘            └──────────────┘
//!         ▲                              │
//!         │credits                packet.messages
//!         │                              ▼
//!     pre-framed packets    itch5::Parser::parse_stream
//!     (seq patched per iter)             │
//!                                        ▼
//!                              Registry<symbol → OrderBook>
//! ```
//!
//! ## Methodology
//!
//! - The sample file is mmap'd once and pre-faulted, then walked once
//!   with `itch5::parse_one` to extract message bodies into a
//!   `Vec<Vec<u8>>` (so wire-format parsing is *not* counted in the
//!   benchmark).
//! - Bodies are pre-framed once into MoldUDP64 packets sized to fit
//!   `MAX_PAYLOAD`. Sequence numbers in the framed bytes start at 1; a
//!   per-iteration offset is patched into the seq field at send time so
//!   the client's expected-seq state remains monotonic across iterations.
//! - A dedicated sender thread owns the send-side `UdpSocket`. It walks
//!   the pre-framed packets in chunks of `CHUNK_PACKETS` and waits on a
//!   credit channel between chunks; the bench thread grants a credit each
//!   time it has drained another `CHUNK_PACKETS` packets. This bounds
//!   in-flight datagrams to `CHUNK_PACKETS * MAX_INFLIGHT_CHUNKS`, which
//!   must stay under the moldudp client data channel capacity (1024) —
//!   the recv thread `try_send`s and silently drops on full, and with no
//!   re-request server wired up a drop would deadlock the bench.
//! - The client downstream socket has `SO_RCVBUF` raised to 8 MiB so the
//!   kernel UDP buffer comfortably absorbs a chunk burst.
//! - Throughput is reported in elements (ITCH messages) and wire bytes
//!   (full MoldUDP64 packet bytes the client sees), so the number reflects
//!   what the client+parser+book pipeline actually consumed.
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
//! Two threads need a CPU: the bench thread (drain + parse + book) and
//! the moldudp client recv thread. The sender thread is also active but
//! mostly blocked on credits; it can share a core. Pinning two cores is
//! a reasonable default:
//!
//! ```sh
//! taskset -c 2-3 cargo bench -p app --bench end_to_end
//!
//! # cap the number of messages replayed (default: all of them):
//! END_TO_END_MSGS=200000 cargo bench -p app --bench end_to_end
//!
//! # use a different sample file:
//! ITCH5_BENCH_FILE=/path/to/itch.bin cargo bench -p app --bench end_to_end
//! ```

use std::cell::Cell;
use std::fs::File;
use std::hint::black_box;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use crossbeam::channel::{self, Receiver, Sender};
use memmap2::Mmap;
use socket2::{Domain, Protocol, Socket, Type};

use itch5::messages::*;
use moldudp::{Datagram, FromBytes, MoldUDP64, Packet, PacketKind, build_packet};
use orderbook::registry::{HashMapRegistry, Registry, VecRegistry};
use orderbook::{Order, Side};

const SESSION: &[u8; 10] = b"BENCHSESHN";
/// Server-side `max_payload` default (1500 MTU − 20 IP − 8 UDP − 20 MoldUDP64
/// header).
const MAX_PAYLOAD: usize = 1452;

/// Packets per credit grant. Must satisfy
/// `CHUNK_PACKETS * MAX_INFLIGHT_CHUNKS < 1024` (moldudp client data
/// channel capacity); see module docs.
const CHUNK_PACKETS: usize = 384;
/// Maximum chunks the sender may have in flight ahead of the consumer.
/// `2` lets the sender stay one chunk ahead so the consumer never waits
/// on a syscall, while keeping the in-flight budget under the channel cap.
const MAX_INFLIGHT_CHUNKS: usize = 2;

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
                if let Some(cap) = limit
                    && out.len() >= cap
                {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    out
}

/// Pre-frame `msgs` into MoldUDP64 packet bytes, byte-batching to mirror
/// what `MoldUDP64Server::Send` would have produced internally. Sequence
/// numbers in the returned bytes start at 1; the sender thread adds a
/// per-iteration offset before transmission.
fn frame_packets(msgs: Vec<Vec<u8>>, max_payload: usize) -> Vec<Vec<u8>> {
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut next_seq: u64 = 1;
    let mut current: Vec<Vec<u8>> = Vec::new();
    let mut current_body: usize = 0;

    for m in msgs {
        if !current.is_empty() && current_body + m.len() > max_payload {
            let count = current.len() as u64;
            packets.push(build_packet(SESSION, next_seq, &current));
            next_seq += count;
            current.clear();
            current_body = 0;
        }
        current_body += m.len();
        current.push(m);
    }
    if !current.is_empty() {
        packets.push(build_packet(SESSION, next_seq, &current));
    }
    packets
}

// ─── Loopback wiring ───────────────────────────────────────────────────────

/// Build the client + sender socket pair. Returns the client's data
/// channel, the sender socket, and the address the sender should target.
fn loopback_setup() -> (Receiver<Datagram>, UdpSocket, SocketAddr) {
    let client_sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).unwrap();
    // Comfortably absorb a `CHUNK_PACKETS` burst even at MTU-sized packets.
    let _ = client_sock.set_recv_buffer_size(8 << 20);
    client_sock
        .bind(&"127.0.0.1:0".parse::<SocketAddr>().unwrap().into())
        .unwrap();
    let client_downstream: UdpSocket = client_sock.into();
    let client_addr = client_downstream.local_addr().unwrap();

    let sender_sock = UdpSocket::bind("127.0.0.1:0").unwrap();

    // The client builder requires at least one re-request server. We replay
    // in order with no gaps, so this address is never contacted.
    let dead_rereq: SocketAddr = "127.0.0.1:1".parse().unwrap();
    let client_rereq = UdpSocket::bind("127.0.0.1:0").unwrap();

    let client = MoldUDP64::builder()
        .multicast_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        .interface_addr(Ipv4Addr::UNSPECIFIED)
        .rerequest_server_addrs(vec![dead_rereq])
        .expected_session_ident(std::str::from_utf8(SESSION).unwrap().to_string())
        .build();

    let (rx, _req_tx) = client
        .start_with_sockets(client_downstream, client_rereq, &[dead_rereq])
        .unwrap();

    (rx, sender_sock, client_addr)
}

// ─── Sender thread ─────────────────────────────────────────────────────────

struct IterCmd {
    /// Added to each packet's original seq before transmission. Lets the
    /// client's expected_seq stay monotonic across bench iterations
    /// without rebuilding the client.
    seq_offset: u64,
    /// Sender consumes one credit before each chunk send.
    credit_rx: Receiver<()>,
    /// Sender signals here once all packets for the iteration are sent.
    done_tx: Sender<()>,
}

fn spawn_sender(
    sock: UdpSocket,
    client_addr: SocketAddr,
    packets: Arc<Vec<Vec<u8>>>,
) -> Sender<IterCmd> {
    let (cmd_tx, cmd_rx) = channel::bounded::<IterCmd>(1);
    let max_pkt = packets.iter().map(|p| p.len()).max().unwrap_or(0);
    thread::spawn(move || {
        let mut scratch = vec![0u8; max_pkt];
        while let Ok(cmd) = cmd_rx.recv() {
            let t0 = std::time::Instant::now();
            let mut sent: usize = 0;
            'outer: for chunk in packets.chunks(CHUNK_PACKETS) {
                if cmd.credit_rx.recv().is_err() {
                    break 'outer;
                }
                for pkt in chunk {
                    scratch.clear();
                    scratch.extend_from_slice(pkt);
                    let orig_seq = u64::from_be_bytes(pkt[10..18].try_into().unwrap());
                    let new_seq = orig_seq + cmd.seq_offset;
                    scratch[10..18].copy_from_slice(&new_seq.to_be_bytes());
                    if let Err(e) = sock.send_to(&scratch, client_addr) {
                        eprintln!("sender: send_to error: {e}");
                        return;
                    }
                    sent += 1;
                }
            }
            eprintln!("sender: sent {sent} pkts in {:?}", t0.elapsed());
            let _ = cmd.done_tx.send(());
        }
    });
    cmd_tx
}

// ─── Replay handler ────────────────────────────────────────────────────────

struct ReplayHandler<R: Registry> {
    registry: R,
    adds: u64,
    execs: u64,
    cancels: u64,
    deletes: u64,
    replaces: u64,
    skipped: u64,
    errors: u64,
}

impl<R: Registry> ReplayHandler<R> {
    fn new(registry: R) -> Self {
        Self {
            registry,
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

impl<R: Registry> itch5::MessageHandler for ReplayHandler<R> {
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

/// Per-iteration state owned by the bench thread: the handler being driven
/// plus the credit/done plumbing for the sender thread.
struct IterState<R: Registry> {
    handler: ReplayHandler<R>,
    credit_tx: Sender<()>,
    done_rx: Receiver<()>,
}

/// Per-iteration setup: build a fresh handler, hand the sender a new credit
/// channel pre-charged with `MAX_INFLIGHT_CHUNKS` credits, and bump the
/// shared seq offset so the moldudp client's expected_seq stays monotonic
/// across iterations *and* across registry parametrizations.
fn setup_iter<R: Registry>(
    registry: R,
    iter_offset: &Cell<u64>,
    total_msgs_u64: u64,
    total_chunks: usize,
    cmd_tx: &Sender<IterCmd>,
) -> IterState<R> {
    let handler = ReplayHandler::new(registry);
    let (credit_tx, credit_rx) = channel::bounded::<()>(MAX_INFLIGHT_CHUNKS);
    let (done_tx, done_rx) = channel::bounded::<()>(1);
    for _ in 0..MAX_INFLIGHT_CHUNKS.min(total_chunks) {
        credit_tx.send(()).unwrap();
    }
    let off = iter_offset.get();
    cmd_tx
        .send(IterCmd {
            seq_offset: off,
            credit_rx,
            done_tx,
        })
        .unwrap();
    iter_offset.set(off + total_msgs_u64);
    IterState {
        handler,
        credit_tx,
        done_rx,
    }
}

/// Per-iteration body: drain `total_msgs` messages out of the moldudp
/// client, parse each packet into book ops, and credit the sender as the
/// in-flight window opens up.
fn run_iter<R: Registry>(
    state: IterState<R>,
    rx: &Receiver<Datagram>,
    total_msgs: usize,
    total_chunks: usize,
) {
    let IterState {
        mut handler,
        credit_tx,
        done_rx,
    } = state;
    let mut delivered: usize = 0;
    let mut delivered_packets: usize = 0;
    let extra_credits_needed = total_chunks.saturating_sub(MAX_INFLIGHT_CHUNKS);
    let mut extra_credits_granted: usize = 0;
    let mut next_credit_at = CHUNK_PACKETS;
    let body_start = std::time::Instant::now();
    let mut first_recv: Option<std::time::Duration> = None;
    let mut recv_time = std::time::Duration::ZERO;
    let mut parse_time = std::time::Duration::ZERO;
    while delivered < total_msgs {
        let t_recv = std::time::Instant::now();
        let dgram = match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(d) => {
                if first_recv.is_none() {
                    first_recv = Some(body_start.elapsed());
                }
                d
            }
            Err(_) => panic!("end_to_end stalled at {delivered}/{total_msgs}"),
        };
        recv_time += t_recv.elapsed();
        let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
        match pkt.packet_kind() {
            PacketKind::Heartbeat | PacketKind::EndOfSession => continue,
            _ => {}
        }
        delivered += pkt.msg_count() as usize;
        delivered_packets += 1;
        let t_parse = std::time::Instant::now();
        itch5::Parser::new(&pkt.messages)
            .parse_stream(&mut handler)
            .unwrap();
        parse_time += t_parse.elapsed();
        if extra_credits_granted < extra_credits_needed && delivered_packets >= next_credit_at {
            let _ = credit_tx.send(());
            extra_credits_granted += 1;
            next_credit_at += CHUNK_PACKETS;
        }
    }
    let drain_done = body_start.elapsed();
    let _ = done_rx.recv_timeout(Duration::from_secs(2));
    let total = body_start.elapsed();
    eprintln!(
        "iter: first_recv={:?} drain_done={:?} total={:?} pkts={delivered_packets} \
         credits_granted={extra_credits_granted}/{extra_credits_needed} \
         recv_total={:?} parse_total={:?}",
        first_recv, drain_done, total, recv_time, parse_time
    );
    black_box((
        handler.adds,
        handler.execs,
        handler.cancels,
        handler.deletes,
        handler.replaces,
        handler.skipped,
        handler.errors,
    ));
}

fn bench_end_to_end(c: &mut Criterion) {
    let Some(mmap) = try_load_sample() else {
        return;
    };

    let limit: Option<usize> = std::env::var("END_TO_END_MSGS")
        .ok()
        .and_then(|s| s.parse().ok());

    let messages = extract_messages(&mmap, limit);
    let total_msgs = messages.len();
    if total_msgs == 0 {
        eprintln!("skip: extracted 0 messages from sample");
        return;
    }
    let packets_vec = frame_packets(messages, MAX_PAYLOAD);
    let total_packet_bytes: u64 = packets_vec.iter().map(|p| p.len() as u64).sum();
    let total_packets = packets_vec.len();
    let packets = Arc::new(packets_vec);

    eprintln!(
        "end_to_end: replaying {total_msgs} ITCH messages across {total_packets} packets \
         ({total_packet_bytes} wire bytes)"
    );

    let (rx, sender_sock, client_addr) = loopback_setup();
    // Let the client multicast loop reach its first `recv` before sending.
    thread::sleep(Duration::from_millis(50));

    let cmd_tx = spawn_sender(sender_sock, client_addr, Arc::clone(&packets));

    let total_chunks = total_packets.div_ceil(CHUNK_PACKETS);
    let total_msgs_u64 = total_msgs as u64;

    let mut g = c.benchmark_group("app/end_to_end");
    g.throughput(Throughput::ElementsAndBytes {
        elements: total_msgs as u64,
        bytes: total_packet_bytes,
    });
    g.sample_size(10);
    g.measurement_time(Duration::from_secs(20));

    // Shared across both registry parametrizations: the moldudp client tracks
    // expected_seq monotonically across its lifetime, so iterations from
    // either bench must use disjoint, increasing seq ranges.
    let iter_offset = Cell::new(0u64);

    g.bench_function(BenchmarkId::new("client→parser→book", "hashmap"), |b| {
        b.iter_batched(
            || {
                setup_iter(
                    HashMapRegistry::new(),
                    &iter_offset,
                    total_msgs_u64,
                    total_chunks,
                    &cmd_tx,
                )
            },
            |state| run_iter(state, &rx, total_msgs, total_chunks),
            BatchSize::PerIteration,
        );
    });

    g.bench_function(BenchmarkId::new("client→parser→book", "vec"), |b| {
        b.iter_batched(
            || {
                setup_iter(
                    VecRegistry::new(),
                    &iter_offset,
                    total_msgs_u64,
                    total_chunks,
                    &cmd_tx,
                )
            },
            |state| run_iter(state, &rx, total_msgs, total_chunks),
            BatchSize::PerIteration,
        );
    });

    g.finish();
}

criterion_group!(benches, bench_end_to_end);
criterion_main!(benches);

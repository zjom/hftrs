//! MoldUDP64 client + packet benchmarks.
//!
//! ## What we measure
//!
//! 1. **Packet wire-format parsing.** Header decode and message iteration
//!    over packets containing 1, 10, 100, and 1000 messages of realistic
//!    ITCH-sized (38 B) payloads. This is the cost a consumer pays once
//!    a datagram is in hand, completely independent of the network.
//! 2. **End-to-end client receive throughput.** A loopback unicast pair
//!    (server → client over [`MoldUDP64::start_with_sockets`]) under
//!    several traffic shapes:
//!    - 1k single-message packets (worst case: header overhead per msg)
//!    - 1k batches of 10 small messages (closer to a live feed)
//!    - 5k batches of 50 ITCH-sized (38 B) messages — best case
//!    - Bytes-only stress: 500 × 1 KiB messages
//! 3. **Gap-detection / retransmission overhead.** A pattern of
//!    drop-then-send to force a steady stream of re-requests; measures
//!    the round-trip cost of detecting, sending, receiving, and merging
//!    a retransmitted packet through the same pipeline.
//!
//! ## Caveats
//!
//! Loopback unicast is *not* a substitute for a real multicast
//! deployment. The numbers here measure the client and server's
//! in-process behavior — packet construction, kernel-loopback transit,
//! receive-side parsing, and channel handoff. They do not reflect NIC
//! offload, kernel-bypass, or real multicast contention. For those, run
//! against an actual exchange tap or a kernel-bypass framework.
//!
//! ## Running
//!
//! ```sh
//! taskset -c 3 cargo bench -p moldudp
//! ```

use std::hint::black_box;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use moldudp::{Datagram, MoldUDP64, MoldUDP64Server};
use moldudp::{FromBytes, Packet, PacketKind};

const SESSION: &str = "BENCHSESHN";

// ─── Loopback fixture ──────────────────────────────────────────────────────

fn loopback_pair() -> (
    crossbeam::channel::Receiver<Datagram>,
    crossbeam::channel::Sender<moldudp::RetransmissionRequest>,
    moldudp::ServerHandle,
) {
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
        .expected_seq_num(1u64)
        .build();

    let (rx, req_tx) = client
        .start_with_sockets(client_downstream, client_rereq, &[rereq_server_addr])
        .unwrap();

    (rx, req_tx, handle)
}

fn drain_data(rx: &crossbeam::channel::Receiver<Datagram>, n: usize) {
    let mut received = 0;
    while received < n {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(dgram) => {
                let pkt = Packet::ref_from_bytes(dgram.bytes()).expect("failed to parse packet");
                match pkt.packet_kind() {
                    PacketKind::Heartbeat | PacketKind::EndOfSession => continue,
                    _ => received += pkt.msg_count() as usize,
                }
            }
            Err(_) => panic!("timed out waiting for messages, got {received}/{n}"),
        }
    }
}

/// Build a synthetic MoldUDP64 packet with the given message count and a
/// fixed 38-byte payload per message (matches an ITCH `A` add-order frame
/// length, the most common message on a real feed).
fn build_packet(msg_count: u16, msg_len: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(20 + (2 + msg_len) * msg_count as usize);
    buf.extend_from_slice(b"BENCHSESHN"); // 10-byte session
    buf.extend_from_slice(&1u64.to_be_bytes()); // seq
    buf.extend_from_slice(&msg_count.to_be_bytes());
    let payload = vec![0xABu8; msg_len];
    let len_be = (msg_len as u16).to_be_bytes();
    for _ in 0..msg_count {
        buf.extend_from_slice(&len_be);
        buf.extend_from_slice(&payload);
    }
    buf
}

// ─── Pure packet parsing ───────────────────────────────────────────────────

/// Header decode + iteration over the message blocks. Varies the message
/// count to expose how iteration cost scales with packet density.
fn bench_packet_parse(c: &mut Criterion) {
    let mut g = c.benchmark_group("moldudp/packet_parse");
    for &count in &[1u16, 10, 100, 1000] {
        let buf = build_packet(count, 38);
        g.throughput(Throughput::Elements(count as u64));
        g.bench_with_input(BenchmarkId::from_parameter(count), &buf, |b, buf| {
            b.iter(|| {
                let pkt = Packet::ref_from_bytes(buf).unwrap();
                let mut len_xor = 0u16;
                let mut byte_xor = 0u8;
                for msg in pkt.iter() {
                    len_xor ^= msg.length();
                    if let Some(&b0) = msg.data().first() {
                        byte_xor ^= b0;
                    }
                }
                black_box((len_xor, byte_xor));
            });
        });
    }
    g.finish();
}

// ─── End-to-end client throughput on loopback ──────────────────────────────

/// Drive the producer on a separate thread and time only the drain.
///
/// The original "send a burst, then drain" shape overflows the kernel UDP
/// receive queue on Linux loopback (rmem_max-bound, no flow control), masking
/// real throughput as packet loss. Running the producer concurrently keeps
/// the receive queue near-empty and matches the steady-state behaviour of a
/// live feed arriving paced over the wire.
fn timed_concurrent_drain<P>(
    rx: &crossbeam::channel::Receiver<Datagram>,
    total_msgs: usize,
    produce: P,
) -> Duration
where
    P: FnOnce() + Send + 'static,
{
    let producer = thread::spawn(produce);
    let start = Instant::now();
    drain_data(rx, total_msgs);
    let elapsed = start.elapsed();
    producer.join().unwrap();
    elapsed
}

fn bench_single_message_throughput(c: &mut Criterion) {
    let mut g = c.benchmark_group("moldudp/client_single_msg");
    let count = 1_000u64;
    g.throughput(Throughput::Elements(count));
    g.sample_size(20);

    g.bench_function("1k_single_messages", |b| {
        let (rx, _req_tx, handle) = loopback_pair();
        b.iter_custom(|iters| {
            let total_packets = count * iters;
            let h = handle.clone();
            timed_concurrent_drain(&rx, total_packets as usize, move || {
                for _ in 0..total_packets {
                    h.send(vec![b"hello world".to_vec()]);
                }
            })
        });
    });
    g.finish();
}

fn bench_batched_messages(c: &mut Criterion) {
    let mut g = c.benchmark_group("moldudp/client_batched");
    let msgs_per_packet = 10u64;
    let packets = 1_000u64;
    let total = packets * msgs_per_packet;
    g.throughput(Throughput::Elements(total));
    g.sample_size(20);

    g.bench_function("1k_packets_x10_msgs", |b| {
        let (rx, _req_tx, handle) = loopback_pair();
        let batch: Vec<Vec<u8>> = (0..msgs_per_packet as usize)
            .map(|_| b"batch-payload".to_vec())
            .collect();
        b.iter_custom(|iters| {
            let total_packets = packets * iters;
            let total_msgs = (total * iters) as usize;
            let h = handle.clone();
            let batch = batch.clone();
            timed_concurrent_drain(&rx, total_msgs, move || {
                for _ in 0..total_packets {
                    h.send(batch.clone());
                }
            })
        });
    });
    g.finish();
}

/// 50 messages × 38 B per packet — close to the densest a real ITCH 5.0
/// feed gets through MoldUDP64 framing under a 1500 B MTU.
fn bench_itch_sized_throughput(c: &mut Criterion) {
    let mut g = c.benchmark_group("moldudp/client_itch_shape");
    let msgs_per_packet = 30usize; // 30 × 38 B + framing fits MTU comfortably
    let packets = 5_000u64;
    let total = packets * msgs_per_packet as u64;
    g.throughput(Throughput::Elements(total));
    g.sample_size(15);

    g.bench_function("5k_packets_x30_x38B", |b| {
        let (rx, _req_tx, handle) = loopback_pair();
        let batch: Vec<Vec<u8>> = (0..msgs_per_packet).map(|_| vec![0xABu8; 38]).collect();
        b.iter_custom(|iters| {
            let total_packets = packets * iters;
            let total_msgs = (total * iters) as usize;
            let h = handle.clone();
            let batch = batch.clone();
            timed_concurrent_drain(&rx, total_msgs, move || {
                for _ in 0..total_packets {
                    h.send(batch.clone());
                }
            })
        });
    });
    g.finish();
}

fn bench_large_messages(c: &mut Criterion) {
    let mut g = c.benchmark_group("moldudp/client_large_msg");
    let count = 500u64;
    let msg_size = 1024;
    g.throughput(Throughput::Bytes(count * msg_size as u64));
    g.sample_size(15);

    g.bench_function("500_x_1KB_messages", |b| {
        let (rx, _req_tx, handle) = loopback_pair();
        let payload = vec![0xABu8; msg_size];
        b.iter_custom(|iters| {
            let total_packets = count * iters;
            let h = handle.clone();
            let payload = payload.clone();
            timed_concurrent_drain(&rx, total_packets as usize, move || {
                for _ in 0..total_packets {
                    h.send(vec![payload.clone()]);
                }
            })
        });
    });
    g.finish();
}

// ─── Gap detection + retransmission ────────────────────────────────────────

fn bench_gap_retransmission(c: &mut Criterion) {
    let mut g = c.benchmark_group("moldudp/client_retransmission");
    g.sample_size(10);

    g.bench_function("50_gaps_retransmitted", |b| {
        let (rx, _req_tx, handle) = loopback_pair();
        b.iter(|| {
            // First message so the client locks on to the session.
            handle.send(vec![b"init".to_vec()]);
            drain_data(&rx, 1);

            // Alternate drop / send to create 50 gaps, each filled by a
            // retransmission round-trip.
            for _ in 0..50 {
                handle.send_dropped(vec![b"dropped".to_vec()]);
                handle.send(vec![b"live".to_vec()]);
            }

            // We expect 50 live + 50 retransmitted = 100.
            let mut received = 0;
            while received < 100 {
                match rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(dgram) => {
                        let pkt =
                            Packet::ref_from_bytes(dgram.bytes()).expect("failed to parse packet");
                        match pkt.packet_kind() {
                            PacketKind::Heartbeat | PacketKind::EndOfSession => continue,
                            _ => received += pkt.msg_count() as usize,
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_packet_parse,
    bench_single_message_throughput,
    bench_batched_messages,
    bench_itch_sized_throughput,
    bench_large_messages,
    bench_gap_retransmission,
);
criterion_main!(benches);

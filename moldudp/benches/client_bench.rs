use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::Duration;
use zerocopy::FromBytes;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use moldudp::{Datagram, MoldUDP64, MoldUDP64Server};
use moldudp::{Packet, PacketKind};

const SESSION: &str = "BENCHSESHN";

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

fn bench_single_message_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("client_single_msg");
    let count = 1_000u64;
    group.throughput(Throughput::Elements(count));
    group.sample_size(20);

    group.bench_function("1k_single_messages", |b| {
        let (rx, _req_tx, handle) = loopback_pair();

        b.iter(|| {
            for _ in 0..count {
                handle.send(vec![b"hello world".to_vec()]);
            }
            drain_data(&rx, count as usize);
        });
    });
    group.finish();
}

fn bench_batched_messages(c: &mut Criterion) {
    let mut group = c.benchmark_group("client_batched");
    let msgs_per_packet = 10;
    let packets = 100u64;
    let total = packets * msgs_per_packet;
    group.throughput(Throughput::Elements(total));
    group.sample_size(20);

    group.bench_function("100_packets_x10_msgs", |b| {
        let (rx, _req_tx, handle) = loopback_pair();
        let batch: Vec<Vec<u8>> = (0..msgs_per_packet as usize)
            .map(|_| b"batch-payload".to_vec())
            .collect();

        b.iter(|| {
            for _ in 0..packets {
                handle.send(batch.clone());
            }
            drain_data(&rx, total as usize);
        });
    });
    group.finish();
}

fn bench_large_messages(c: &mut Criterion) {
    let mut group = c.benchmark_group("client_large_msg");
    let count = 500u64;
    let msg_size = 1024;
    group.throughput(Throughput::Bytes(count * msg_size as u64));
    group.sample_size(20);

    group.bench_function("500_x_1KB_messages", |b| {
        let (rx, _req_tx, handle) = loopback_pair();
        let payload = vec![0xABu8; msg_size];

        b.iter(|| {
            for _ in 0..count {
                handle.send(vec![payload.clone()]);
            }
            drain_data(&rx, count as usize);
        });
    });
    group.finish();
}

fn bench_gap_retransmission(c: &mut Criterion) {
    let mut group = c.benchmark_group("client_retransmission");
    group.sample_size(10);

    group.bench_function("50_gaps_retransmitted", |b| {
        let (rx, _req_tx, handle) = loopback_pair();

        b.iter(|| {
            // Send first message so client locks on
            handle.send(vec![b"init".to_vec()]);
            drain_data(&rx, 1);

            // Alternate: drop one, send one — creates 50 gaps
            for _ in 0..50 {
                handle.send_dropped(vec![b"dropped".to_vec()]);
                handle.send(vec![b"live".to_vec()]);
            }

            // We should receive 50 live + 50 retransmitted = 100 messages
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
    group.finish();
}

fn bench_packet_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_parsing");

    // Build a packet with 100 messages
    let mut buf = Vec::new();
    buf.extend_from_slice(b"0123456789"); // session
    buf.extend_from_slice(&1u64.to_be_bytes()); // seq
    let msg_count = 100u16;
    buf.extend_from_slice(&msg_count.to_be_bytes());
    for i in 0..msg_count {
        let payload = format!("message-{i:04}");
        buf.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        buf.extend_from_slice(payload.as_bytes());
    }

    group.throughput(Throughput::Elements(msg_count as u64));

    group.bench_function("iterate_100_messages", |b| {
        b.iter(|| {
            let pkt = Packet::ref_from_bytes(&buf).expect("failed to parse packet");
            let mut count = 0u16;
            for msg in pkt.iter() {
                std::hint::black_box(msg.data());
                count += 1;
            }
            assert_eq!(count, msg_count);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_packet_parsing,
    bench_single_message_throughput,
    bench_batched_messages,
    bench_large_messages,
    bench_gap_retransmission,
);
criterion_main!(benches);

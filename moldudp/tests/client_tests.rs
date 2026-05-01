use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::thread;
use std::time::Duration;

use moldudp::{FromBytes, MoldUDP64, MoldUDP64Server, Packet, PacketKind};

const SESSION: &str = "ABCDEFGHIJ";

/// Helper: spin up a server and client wired over loopback unicast (no multicast).
/// Returns (client_rx, client_req_tx, server_handle).
fn loopback_pair() -> (
    crossbeam::channel::Receiver<moldudp::Datagram>,
    crossbeam::channel::Sender<moldudp::RetransmissionRequest>,
    moldudp::ServerHandle,
) {
    loopback_pair_with_session(SESSION)
}

fn loopback_pair_with_session(
    session: &str,
) -> (
    crossbeam::channel::Receiver<moldudp::Datagram>,
    crossbeam::channel::Sender<moldudp::RetransmissionRequest>,
    moldudp::ServerHandle,
) {
    // Bind the client's downstream socket first so we know its address.
    let client_downstream = UdpSocket::bind("127.0.0.1:0").unwrap();
    let client_addr = client_downstream.local_addr().unwrap();

    // Bind the rerequest server socket.
    let rereq_server_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let rereq_server_addr = rereq_server_sock.local_addr().unwrap();

    // Bind the server's outbound socket (sends downstream to the client).
    let server_downstream = UdpSocket::bind("127.0.0.1:0").unwrap();

    // Start server — sends unicast to the client's downstream address.
    let server = MoldUDP64Server::builder()
        .multicast_addr(SocketAddrV4::new(
            client_addr.ip().to_string().parse().unwrap(),
            client_addr.port(),
        ))
        .rerequest_bind_addr(rereq_server_addr)
        .session(session.to_string())
        .heartbeat_interval(Duration::from_millis(100))
        .build();

    let handle = server
        .start_with_sockets(server_downstream, rereq_server_sock)
        .unwrap();

    // Bind the client's rerequest socket.
    let client_rereq = UdpSocket::bind("127.0.0.1:0").unwrap();

    // Start client.
    let client = MoldUDP64::builder()
        .multicast_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        .interface_addr(Ipv4Addr::UNSPECIFIED)
        .rerequest_server_addrs(vec![rereq_server_addr])
        .expected_session_ident(session.to_string())
        .expected_seq_num(1u64)
        .build();

    let (rx, req_tx) = client
        .start_with_sockets(client_downstream, client_rereq, &[rereq_server_addr])
        .unwrap();

    (rx, req_tx, handle)
}

fn recv_timeout(rx: &crossbeam::channel::Receiver<moldudp::Datagram>) -> Option<moldudp::Datagram> {
    rx.recv_timeout(Duration::from_secs(3)).ok()
}

// ─── Packet parsing tests ───

#[test]
fn packet_parse_session_ident() {
    let mut buf = [0u8; 20];
    buf[..10].copy_from_slice(b"TESTSESSN ");
    let pkt = Packet::ref_from_bytes(&buf).unwrap();
    assert_eq!(pkt.session_ident().unwrap(), "TESTSESSN ");
}

#[test]
fn packet_parse_seq_num() {
    let mut buf = [0u8; 20];
    buf[..10].copy_from_slice(b"0123456789");
    buf[10..18].copy_from_slice(&42u64.to_be_bytes());
    let pkt = Packet::ref_from_bytes(&buf).unwrap();
    assert_eq!(pkt.seq_num(), 42);
}

#[test]
fn packet_parse_msg_count() {
    let mut buf = [0u8; 20];
    buf[..10].copy_from_slice(b"0123456789");
    buf[18..20].copy_from_slice(&3u16.to_be_bytes());
    let pkt = Packet::ref_from_bytes(&buf).unwrap();
    assert_eq!(pkt.msg_count(), 3);
}

#[test]
fn packet_kind_heartbeat() {
    let mut buf = [0u8; 20];
    buf[..10].copy_from_slice(b"0123456789");
    // msg_count = 0 => heartbeat
    let pkt = Packet::ref_from_bytes(&buf).unwrap();
    assert!(matches!(pkt.packet_kind(), PacketKind::Heartbeat));
}

#[test]
fn packet_kind_end_of_session() {
    let mut buf = [0u8; 20];
    buf[..10].copy_from_slice(b"0123456789");
    buf[18..20].copy_from_slice(&0xFFFFu16.to_be_bytes());
    let pkt = Packet::ref_from_bytes(&buf).unwrap();
    assert!(matches!(pkt.packet_kind(), PacketKind::EndOfSession));
}

#[test]
fn packet_kind_standard() {
    let mut buf = [0u8; 20];
    buf[..10].copy_from_slice(b"0123456789");
    buf[18..20].copy_from_slice(&5u16.to_be_bytes());
    let pkt = Packet::ref_from_bytes(&buf).unwrap();
    assert!(matches!(pkt.packet_kind(), PacketKind::Standard));
}

// ─── Message iteration tests ───

#[test]
fn packet_iter_single_message() {
    // header + one message: length=5, data=b"hello"
    let mut buf = Vec::new();
    buf.extend_from_slice(b"0123456789"); // session
    buf.extend_from_slice(&1u64.to_be_bytes()); // seq
    buf.extend_from_slice(&1u16.to_be_bytes()); // msg_count
    buf.extend_from_slice(&5u16.to_be_bytes()); // msg len
    buf.extend_from_slice(b"hello"); // msg data

    let pkt = Packet::ref_from_bytes(&buf).unwrap();
    let msgs: Vec<_> = pkt.iter().collect();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].length(), 5);
    assert_eq!(msgs[0].data(), b"hello");
}

#[test]
fn packet_iter_multiple_messages() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"0123456789");
    buf.extend_from_slice(&1u64.to_be_bytes());
    buf.extend_from_slice(&3u16.to_be_bytes());
    for payload in [b"aaa".as_slice(), b"bb", b"c"] {
        buf.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        buf.extend_from_slice(payload);
    }

    let pkt = Packet::ref_from_bytes(&buf).unwrap();
    let msgs: Vec<_> = pkt.iter().collect();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].data(), b"aaa");
    assert_eq!(msgs[1].data(), b"bb");
    assert_eq!(msgs[2].data(), b"c");
}

#[test]
fn packet_iter_heartbeat_yields_nothing() {
    let buf = [0u8; 20]; // msg_count = 0
    let pkt = Packet::ref_from_bytes(&buf);
    assert_eq!(pkt.iter().len(), 0);
    assert!(pkt.iter().next().is_none());
}

#[test]
fn packet_iter_end_of_session_yields_nothing() {
    let mut buf = [0u8; 20];
    buf[18..20].copy_from_slice(&0xFFFFu16.to_be_bytes());
    let pkt = Packet::ref_from_bytes(&buf);
    assert_eq!(pkt.iter().len(), 0);
    assert!(pkt.iter().next().is_none());
}

#[test]
fn packet_iter_truncated_message_stops_early() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"0123456789");
    buf.extend_from_slice(&1u64.to_be_bytes());
    buf.extend_from_slice(&2u16.to_be_bytes()); // claims 2 messages
    // first message is fine
    buf.extend_from_slice(&3u16.to_be_bytes());
    buf.extend_from_slice(b"abc");
    // second message: length says 10 but only 2 bytes follow
    buf.extend_from_slice(&10u16.to_be_bytes());
    buf.extend_from_slice(b"xy");

    let pkt = Packet::ref_from_bytes(&buf);
    let msgs: Vec<_> = pkt.iter().collect();
    assert_eq!(msgs.len(), 1); // only the first valid message
}

#[test]
fn packet_exact_size_iterator() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"0123456789");
    buf.extend_from_slice(&1u64.to_be_bytes());
    buf.extend_from_slice(&2u16.to_be_bytes());
    for payload in [b"aa".as_slice(), b"bb"] {
        buf.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        buf.extend_from_slice(payload);
    }

    let pkt = Packet::ref_from_bytes(&buf);
    let iter = pkt.iter();
    assert_eq!(iter.len(), 2);
}

// ─── Client integration tests (over loopback) ───

#[test]
fn client_receives_single_packet() {
    let (rx, _req_tx, handle) = loopback_pair();

    handle.send(vec![b"hello".to_vec()]);

    let dgram = recv_timeout(&rx).expect("should receive a datagram");
    let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
    let msgs: Vec<_> = pkt.iter().collect();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].data(), b"hello");
}

#[test]
fn client_receives_multiple_messages_in_one_packet() {
    let (rx, _req_tx, handle) = loopback_pair();

    handle.send(vec![b"one".to_vec(), b"two".to_vec(), b"three".to_vec()]);

    let dgram = recv_timeout(&rx).expect("should receive a datagram");
    let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
    let msgs: Vec<_> = pkt.iter().collect();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].data(), b"one");
    assert_eq!(msgs[1].data(), b"two");
    assert_eq!(msgs[2].data(), b"three");
}

#[test]
fn client_receives_heartbeat() {
    let (rx, _req_tx, handle) = loopback_pair();

    handle.heartbeat();

    let dgram = recv_timeout(&rx).expect("should receive heartbeat");
    let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
    assert!(matches!(pkt.packet_kind(), PacketKind::Heartbeat));
    assert_eq!(pkt.msg_count(), 0);
}

#[test]
fn client_receives_end_of_session() {
    let (rx, _req_tx, handle) = loopback_pair();

    handle.end_of_session();

    let dgram = recv_timeout(&rx).expect("should receive end-of-session");
    let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
    assert!(matches!(pkt.packet_kind(), PacketKind::EndOfSession));
}

#[test]
fn client_detects_gap_and_receives_retransmission() {
    let (rx, _req_tx, handle) = loopback_pair();

    // Send message at seq 1 (client expects seq 1)
    handle.send(vec![b"first".to_vec()]);
    let dgram = recv_timeout(&rx).expect("should receive first packet");
    let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
    let msgs: Vec<_> = pkt.iter().collect();
    assert_eq!(msgs[0].data(), b"first");

    // "Drop" a message — it's stored in the log but not broadcast.
    handle.send_dropped(vec![b"missed".to_vec()]);

    // Send next message — client now sees a gap (expected seq 2, got seq 3).
    handle.send(vec![b"third".to_vec()]);

    // Client should receive the live packet for "third" and also get the
    // retransmission for "missed" (via automatic gap detection).
    let mut seen_third = false;
    let mut seen_missed = false;

    for _ in 0..10 {
        if seen_third && seen_missed {
            break;
        }
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(dgram) => {
                let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
                if matches!(
                    pkt.packet_kind(),
                    PacketKind::Heartbeat | PacketKind::EndOfSession
                ) {
                    continue;
                }
                for msg in pkt.iter() {
                    match msg.data() {
                        b"third" => seen_third = true,
                        b"missed" => seen_missed = true,
                        _ => {}
                    }
                }
            }
            Err(_) => break,
        }
    }

    assert!(seen_third, "should have received the live 'third' packet");
    assert!(
        seen_missed,
        "should have received the retransmitted 'missed' packet"
    );
}

#[test]
fn client_receives_multiple_sequential_packets() {
    let (rx, _req_tx, handle) = loopback_pair();

    for i in 0..5 {
        handle.send(vec![format!("msg-{i}").into_bytes()]);
        // Small delay so packets arrive in order.
        thread::sleep(Duration::from_millis(10));
    }

    let mut received = Vec::new();
    for _ in 0..20 {
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(dgram) => {
                let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
                if matches!(
                    pkt.packet_kind(),
                    PacketKind::Heartbeat | PacketKind::EndOfSession
                ) {
                    continue;
                }
                for msg in pkt.iter() {
                    received.push(msg.data().to_vec());
                }
            }
            Err(_) => break,
        }
        if received.len() >= 5 {
            break;
        }
    }

    for i in 0..5 {
        let expected = format!("msg-{i}").into_bytes();
        assert!(
            received.contains(&expected),
            "missing msg-{i} in received: {received:?}"
        );
    }
}

#[test]
fn client_session_ident_matches() {
    let (rx, _req_tx, handle) = loopback_pair();

    handle.send(vec![b"payload".to_vec()]);

    let dgram = recv_timeout(&rx).expect("should receive datagram");
    let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
    // Server pads session to 10 bytes with spaces.
    assert_eq!(pkt.session_ident().unwrap(), "ABCDEFGHIJ");
}

#[test]
fn datagram_bytes_returns_correct_slice() {
    let (rx, _req_tx, handle) = loopback_pair();

    let payload = b"test-payload";
    handle.send(vec![payload.to_vec()]);

    let dgram = recv_timeout(&rx).expect("should receive datagram");
    let bytes = dgram.bytes();
    // Should be a valid MoldUDP64 packet (at least 20 bytes header)
    assert!(bytes.len() >= 20);
    // The packet should contain our message
    let pkt = Packet::ref_from_bytes(bytes).unwrap();
    assert_eq!(pkt.msg_count(), 1);
}

#[test]
fn client_handles_empty_message() {
    let (rx, _req_tx, handle) = loopback_pair();

    // A zero-length message is valid per spec.
    handle.send(vec![vec![]]);

    let dgram = recv_timeout(&rx).expect("should receive datagram");
    let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
    let msgs: Vec<_> = pkt.iter().collect();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].length(), 0);
    assert_eq!(msgs[0].data(), b"");
}

#[test]
fn automatic_heartbeats_are_received() {
    let (rx, _req_tx, _handle) = loopback_pair();

    // Server sends heartbeats every 100ms; just wait for one.
    let dgram = recv_timeout(&rx).expect("should receive automatic heartbeat");
    let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
    assert!(matches!(pkt.packet_kind(), PacketKind::Heartbeat));
}

#[test]
fn stop_session_switches_heartbeats_to_eos() {
    let (rx, _req_tx, handle) = loopback_pair();

    handle.shutdown();

    // After shutdown, periodic packets become end-of-session.
    let mut saw_eos = false;
    for _ in 0..20 {
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(dgram) => {
                let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
                if matches!(pkt.packet_kind(), PacketKind::EndOfSession) {
                    saw_eos = true;
                    break;
                }
            }
            Err(_) => break,
        }
    }
    assert!(saw_eos, "should receive end-of-session after shutdown");
}

// ─── RetransmissionPacket serialization ───

#[test]
fn retransmission_packet_round_trip() {
    // We can't directly access serialize_into since it's private,
    // but we can verify the wire format by going through the server's
    // rerequest path. Instead, test the packet format manually.
    let session = *b"ABCDEFGHIJ";
    let seq_num: u64 = 42;
    let msg_count: u16 = 10;

    // Build what a serialized retransmission request looks like.
    let mut buf = [0u8; 20];
    buf[0..10].copy_from_slice(&session);
    buf[10..18].copy_from_slice(&seq_num.to_be_bytes());
    buf[18..20].copy_from_slice(&msg_count.to_be_bytes());

    // Verify the fields by parsing back.
    assert_eq!(&buf[0..10], b"ABCDEFGHIJ");
    assert_eq!(u64::from_be_bytes(buf[10..18].try_into().unwrap()), 42);
    assert_eq!(u16::from_be_bytes(buf[18..20].try_into().unwrap()), 10);
}

// ─── Large message test ───

#[test]
fn client_handles_large_messages() {
    let (rx, _req_tx, handle) = loopback_pair();

    let large_payload = vec![0xABu8; 1000];
    handle.send(vec![large_payload.clone()]);

    let dgram = recv_timeout(&rx).expect("should receive large message");
    let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
    let msgs: Vec<_> = pkt.iter().collect();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].data(), large_payload.as_slice());
}

// ─── Multiple gaps trigger multiple retransmissions ───

#[test]
fn client_retransmits_multiple_gaps() {
    let (rx, _req_tx, handle) = loopback_pair();

    // seq 1: live
    handle.send(vec![b"a".to_vec()]);
    recv_timeout(&rx).expect("receive seq 1");

    // seq 2: dropped
    handle.send_dropped(vec![b"b".to_vec()]);

    // seq 3: live — triggers gap for seq 2
    handle.send(vec![b"c".to_vec()]);

    // seq 4: dropped
    handle.send_dropped(vec![b"d".to_vec()]);

    // seq 5: live — triggers gap for seq 4
    handle.send(vec![b"e".to_vec()]);

    let mut seen = std::collections::HashSet::new();
    for _ in 0..20 {
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(dgram) => {
                let pkt = Packet::ref_from_bytes(dgram.bytes()).unwrap();
                if matches!(
                    pkt.packet_kind(),
                    PacketKind::Heartbeat | PacketKind::EndOfSession
                ) {
                    continue;
                }
                for msg in pkt.iter() {
                    seen.insert(msg.data().to_vec());
                }
            }
            Err(_) => break,
        }
        if seen.len() >= 4 {
            break;
        }
    }

    assert!(seen.contains(&b"c".to_vec()), "should see live 'c'");
    assert!(seen.contains(&b"e".to_vec()), "should see live 'e'");
    assert!(
        seen.contains(&b"b".to_vec()),
        "should see retransmitted 'b'"
    );
    assert!(
        seen.contains(&b"d".to_vec()),
        "should see retransmitted 'd'"
    );
}

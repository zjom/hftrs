# moldudp

A Rust implementation of the [MoldUDP64](https://www.nasdaqtrader.com/content/technicalsupport/specifications/dataproducts/moldudp64.pdf) protocol — a lightweight UDP transport for reliable, ordered market-data feeds.

## Features

- Automatic gap detection and transparent re-request of missing packets
- Load-balanced re-requests across multiple servers via MPMC channel
- Configurable per-request retry limit
- Zero-copy packet and message types (no allocation on the hot path)
- Lock-free buffer pool for receive buffers
- Included test server for integration testing and local development

## Installation

```toml
[dependencies]
moldudp = "0.1"
```

## Quick start

```rust
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use moldudp::{FromBytes, MoldUDP64, Packet, PacketKind};

let (rx, _req_tx) = MoldUDP64::builder()
    .multicast_addr(SocketAddrV4::new(Ipv4Addr::new(233, 252, 0, 1), 30001))
    .interface_addr(Ipv4Addr::UNSPECIFIED)
    .rerequest_server_addrs(vec![
        SocketAddr::from(([10, 0, 0, 1], 30002)),
    ])
    .build()
    .start()?;

while let Ok(datagram) = rx.recv() {
    let packet = Packet::ref_from_bytes(datagram.bytes()).unwrap();

    match packet.packet_kind() {
        PacketKind::Heartbeat | PacketKind::EndOfSession => continue,
        PacketKind::Standard => {}
    }

    for msg in packet.iter() {
        handle(msg.data());
    }
}
```

## Usage

### Client configuration

```rust
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use moldudp::{FromBytes, MoldUDP64, Packet, PacketKind,
              RetransmissionPacket, RetransmissionRequest};

let (rx, req_tx) = MoldUDP64::builder()
    // Multicast group + port carrying the live downstream feed.
    .multicast_addr(SocketAddrV4::new(Ipv4Addr::new(233, 252, 0, 1), 30001))
    // Local NIC to join on. `UNSPECIFIED` lets the OS choose.
    .interface_addr(Ipv4Addr::UNSPECIFIED)
    // One or more unicast re-request servers.
    // Requests are load-balanced across all entries.
    .rerequest_server_addrs(vec![
        SocketAddr::from(([10, 0, 0, 1], 30002)),
        SocketAddr::from(([10, 0, 0, 2], 30002)),
    ])
    // Optional: lock onto a specific session. Packets from any other
    // session ident are dropped. Omit to lock onto the first session seen.
    .expected_session_ident("0123456789".to_string())
    // Optional: first sequence number of interest. Gaps before this are
    // not re-requested.
    .expected_seq_num(1)
    // Optional: max send failures per re-request before giving up.
    .max_rerequest_retries(10)
    .build()
    .start()?;
```

### Receiving and inspecting packets

Datagrams arrive on the channel in *receive order* — live and retransmitted
packets are interleaved. The consumer is responsible for reordering by
`(session_ident, seq_num)`.

```rust
while let Ok(datagram) = rx.recv() {
    // Zero-copy view — no allocation.
    let packet = Packet::ref_from_bytes(datagram.bytes()).unwrap();

    match packet.packet_kind() {
        PacketKind::Heartbeat | PacketKind::EndOfSession => continue,
        PacketKind::Standard => {}
    }

    println!(
        "session={} seq={} messages={}",
        packet.session_ident().unwrap_or("?"),
        packet.seq_num(),
        packet.msg_count(),
    );

    for msg in packet.iter() {
        // msg.data() is a zero-copy slice into datagram's buffer.
        process(msg.data());
    }
}
```

### Manual re-requests

The client detects gaps automatically, but you can also inject re-requests
manually — for example, if your application detects a corrupt or incomplete
packet:

```rust
use moldudp::{RetransmissionPacket, RetransmissionRequest};

let rereq = RetransmissionPacket {
    session: *packet.session_ident_raw(),
    seq_num: packet.seq_num().into(),
    msg_count: packet.msg_count().into(),
};
req_tx.try_send(RetransmissionRequest::new(rereq))?;
```

## Architecture

`MoldUDP64::start` spawns `2 + N` threads (where *N* = number of re-request
servers):

```
  Multicast socket          Re-request servers
       │                        │   │
       ▼                        │   │
  [mcast recv]  ──gap req──▶  [req senders]
       │                        │
       │         ◀──retx──  [req recv]
       ▼               │
   data channel ◀──────┘
       │
       ▼
   consumer
```

1. **Multicast receiver** — reads the live downstream feed, detects sequence
   gaps, and enqueues `RetransmissionRequest`s automatically.
2. **Re-request senders** (one per server) — compete on a shared MPMC channel
   so load is spread across servers without any coordination overhead.
3. **Re-request receiver** — reads responses from all servers on the shared
   unicast socket and forwards them into the same data channel as live packets.

## Packet format

Every MoldUDP64 packet starts with a 20-byte header followed by zero or more
length-prefixed message blocks:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
├─────────────────────────────────────────────────────────────────┤
│               Session Identifier (10 bytes, ASCII)              │
├─────────────────────────────────────────────────────────────────┤
│              Sequence Number (8 bytes, big-endian u64)          │
├────────────────────────────────┬────────────────────────────────┤
│  Message Count (2 bytes, u16)  │  Message Length (2 bytes, u16) │
├────────────────────────────────┴────────────────────────────────┤
│                    Message Payload (variable)                    │
├─────────────────────────────────────────────────────────────────┤
│                         … more messages …                        │
└─────────────────────────────────────────────────────────────────┘
```

Special `msg_count` values:

| Value    | Meaning        | Notes                                         |
|----------|----------------|-----------------------------------------------|
| `0`      | Heartbeat      | Carries `seq_num` of next expected message    |
| `0xFFFF` | End of session | Last chance to re-request; no new messages    |
| other    | Standard       | Followed by that many message blocks          |

## Test server

`MoldUDP64Server` provides a reference server for integration tests and
local development:

```rust
use moldudp::MoldUDP64Server;

let server = MoldUDP64Server::builder()
    .multicast_addr("239.1.2.3:5000".parse()?)
    .rerequest_bind_addr("127.0.0.1:6000".parse()?)
    .session("TESTSESSN")
    .build();

let h = server.start()?;

h.send(vec![b"msg1".to_vec()]);            // seq 1 — broadcast live
h.send_dropped(vec![b"msg2".to_vec()]);    // seq 2 — stored only (induces gap)
h.send(vec![b"msg3".to_vec()]);            // seq 3 — triggers gap detection

// Heartbeats are sent automatically every second.
// Call shutdown() when done; end-of-session replaces heartbeats.
h.shutdown();
```

## Performance

- **Zero allocation on the receive path** — packets are parsed as
  [`zerocopy`](https://docs.rs/zerocopy) DSTs directly over pooled buffers.
- **Pre-allocated buffer pool** — 1 024 × 512 KiB buffers. Buffers are
  returned to the pool when `Datagram` is dropped.

## Roadmap

- [ ] Zero-copy reads directly from the network socket (kernel bypass / `io_uring`)

# MoldUDP64 Client for Rust

A blazingly fast [MoldUDP64](https://www.nasdaqtrader.com/content/technicalsupport/specifications/dataproducts/moldudp64.pdf) client in Rust.

## Features

- Automatically re-requests dropped/ missing packets
- Multiple re-request servers
- Automatically retries failed re-requests with configurable limit.
- Lock free
- ZERO hot path allocations
- Allocation free data structures to inspect packets.

## Usage

`cargo add moldudp`

```rust
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use moldudp::{MoldUDP64,Packet,RetransmissionPacket,RetransmissionRequest,PacketKind};

let (rx, tx) = MoldUDP64::builder()
    // Multicast group + port carrying the live downstream feed.
    .multicast_addr(SocketAddrV4::new(Ipv4Addr::new(233, 252, 0, 1), 30001))
    // Local NIC to join on. Use `UNSPECIFIED` to let the OS pick.
    .interface_addr(Ipv4Addr::UNSPECIFIED)
    // One or more re-request servers. The client load-balances requests
    // across them and merges responses back into the same stream.
    .rerequest_server_addrs(vec![
        SocketAddr::from(([10, 0, 0, 1], 30002)),
        SocketAddr::from(([10, 0, 0, 2], 30002)),
    ])
    // Optional: pin the expected session. If set, packets from any other
    // session ident are dropped. If omitted, the client locks onto the
    // first session it sees.
    .expected_session_ident("0123456789".to_string())
    // Optional: starting sequence number. Defaults to 1 (the start of
    // the session) if omitted; gaps before this point are not requested.
    .expected_seq_num(1)
    .build()
    .start()?;

// Datagrams arrive in receive order — live and retransmitted packets are
// interleaved. The consumer is responsible for ordering by seq num.
// Minimal validation is done on datagrams, only that they are at least
// 20 bytes in length.
while let Ok(datagram) = rx.recv() {
    // Use [`moldudp::Packet`] to construct a 0 allocation view on the bytes.
    let packet = Packet::new(datagram.bytes());

    // simple validation of messages
    if packet.packet_kind() == PacketKind::Heartbeat ||
       packet.packet_kind() == PacketKind::EndOfSession {
       continue
    }
    
    if packet.iter().len() != packet.msg_count() {
        let rereq = RetransmissionPacket{
            session: packet.session_ident_raw(),
            seq_num: packet.seq_num(),
            msg_count: packet.msg_count()
        }
        tx.try_send(RetransmissionRequest::new(rereq))?
    }

    handle(packet);
}
```


## Roadmap

- [ ] Zero copy reads from network socket

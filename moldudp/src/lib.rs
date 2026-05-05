//! MoldUDP64 client and test server.
//!
//! [MoldUDP64](https://www.nasdaqtrader.com/content/technicalsupport/specifications/dataproducts/moldudp64.pdf)
//! is a lightweight UDP-based protocol for reliable, ordered distribution of
//! sequenced market-data streams. A downstream multicast channel carries
//! live packets; a unicast re-request server answers gap-fill queries,
//! allowing receivers to reconstruct a lossless, gapless message stream
//! out of an inherently lossy UDP transport.
//!
//! This crate provides:
//!
//! - [`MoldUDP64`] — a multi-threaded client that joins the downstream
//!   multicast group, detects sequence gaps, and transparently re-requests
//!   missing packets from one or more unicast servers.
//!
//! - [`MoldUDP64Server`] — a test/development server that broadcasts packets
//!   over multicast (or unicast in unit tests) and answers re-request queries.
//!
//! - Zero-copy packet types ([`Packet`], [`Message`], [`Messages`]) backed by
//!   [`zerocopy`] DSTs, so you can inspect wire bytes without any allocation.
//!
//! # Client threading model
//!
//! [`MoldUDP64::start`] spawns `2 + N` threads (where *N* is the number of
//! configured re-request servers):
//!
//! 1. **Multicast receiver** — reads downstream datagrams from the multicast
//!    socket, detects sequence gaps, enqueues [`RetransmissionRequest`]s, and
//!    forwards every received datagram to the data channel.
//!
//! 2. **Re-request senders** (one per server) — compete for items on the
//!    same MPMC request channel. Whichever sender is idle picks up the next
//!    request, spreading load across all servers and preventing head-of-line
//!    blocking from a slow peer. Failed sends are retried up to
//!    `max_rerequest_retries` times.
//!
//! 3. **Re-request receiver** — reads retransmission responses from the
//!    shared unicast socket and merges them into the same data channel as live
//!    packets. Retransmitted packets do *not* advance the gap-detection state;
//!    they arrive out-of-order relative to the live stream.
//!
//! Both live and retransmitted packets arrive on the [`crossbeam`] data
//! channel in *receive order*. The consumer is responsible for reordering by
//! `(session_ident, seq_num)`.
//!
//! # Quick start
//!
//! ```no_run
//! use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
//! use moldudp::{FromBytes, MoldUDP64, Packet, PacketKind,
//!               RetransmissionPacket, RetransmissionRequest};
//!
//! let (rx, req_tx) = MoldUDP64::builder()
//!     // Multicast group and port carrying the live downstream feed.
//!     .multicast_addr(SocketAddrV4::new(Ipv4Addr::new(233, 252, 0, 1), 30001))
//!     // Local NIC to join on; UNSPECIFIED lets the OS choose.
//!     .interface_addr(Ipv4Addr::UNSPECIFIED)
//!     // One or more unicast re-request servers.
//!     .rerequest_server_addrs(vec![
//!         SocketAddr::from(([10, 0, 0, 1], 30002)),
//!         SocketAddr::from(([10, 0, 0, 2], 30002)),
//!     ])
//!     .build()
//!     .start()
//!     .unwrap();
//!
//! while let Ok(datagram) = rx.recv() {
//!     // Zero-copy view — no allocation.
//!     let packet = Packet::ref_from_bytes(datagram.bytes()).unwrap();
//!
//!     match packet.packet_kind() {
//!         PacketKind::Heartbeat | PacketKind::EndOfSession => continue,
//!         PacketKind::Standard => {}
//!     }
//!
//!     for msg in packet.iter() {
//!         println!("seq={} payload={:?}", packet.seq_num(), msg.data());
//!     }
//! }
//! ```
//!
//! # Packet format
//!
//! Every MoldUDP64 packet begins with a fixed 20-byte [`PacketHeader`]:
//!
//! ```text
//! Offset  Size  Field
//! ──────────────────────────────────────────────────────
//!  0       10   Session identifier (ASCII, space-padded)
//! 10        8   Sequence number of first message (big-endian u64)
//! 18        2   Message count (big-endian u16)
//! ──────────────────────────────────────────────────────
//! ```
//!
//! The header is followed by zero or more [`Message`] blocks, each
//! consisting of a 2-byte big-endian length prefix and the message payload.
//! Two special `msg_count` values carry no message data:
//!
//! - `0` ([`HEARTBEAT_IDENT`]) — periodic liveness signal.
//! - `0xFFFF` ([`END_OF_SESSION_IDENT`]) — session has ended; last chance to
//!   re-request.

mod client;
mod errors;
mod packet;
pub use client::{Datagram, MoldUDP64, RetransmissionPacket, RetransmissionRequest};
pub use errors::MoldUdpError;
pub use packet::*;
mod server;
pub use server::{MoldUDP64Server, RawPacket, ServerHandle, build_packet};
mod util;

pub use zerocopy::FromBytes;

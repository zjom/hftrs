use crossbeam::channel::{self, Receiver, Sender};
use crossbeam::queue::ArrayQueue;
use std::ops::Deref;
use std::{
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    sync::Arc,
    thread::spawn,
};
use tracing::{debug, error, info, info_span, trace, warn};
use zerocopy::{FromBytes, IntoBytes};

use crate::packet::{Packet, PacketHeader};
use crate::util::pad_session;
use bon::Builder;

/// MoldUDP64 client.
///
/// Subscribes to the downstream multicast group and transparently re-requests
/// any missed packets from one or more unicast re-request servers. Both live
/// and retransmitted downstream packets are surfaced through a single
/// [`crossbeam`] channel of [`Datagram`]s. The consumer is responsible for
/// reordering by `(session_ident, seq_num)`.
///
/// # Example
///
/// ```no_run
/// use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
/// use moldudp::{FromBytes, MoldUDP64, Packet, PacketKind,
///               RetransmissionPacket, RetransmissionRequest};
///
/// let (rx, req_tx) = MoldUDP64::builder()
///     // Multicast group + port carrying the live downstream feed.
///     .multicast_addr(SocketAddrV4::new(Ipv4Addr::new(233, 252, 0, 1), 30001))
///     // Local NIC to join on. Use `UNSPECIFIED` to let the OS pick.
///     .interface_addr(Ipv4Addr::UNSPECIFIED)
///     // One or more re-request servers. The client load-balances requests
///     // across them and merges responses back into the same stream.
///     .rerequest_server_addrs(vec![
///         SocketAddr::from(([10, 0, 0, 1], 30002)),
///         SocketAddr::from(([10, 0, 0, 2], 30002)),
///     ])
///     // Optional: pin to a known session. Packets from any other session
///     // ident are dropped. Omit to lock onto the first session seen.
///     .expected_session_ident("0123456789".to_string())
///     // Optional: first sequence number of interest. Gaps before this
///     // point are not re-requested. Defaults to the start of the stream.
///     .expected_seq_num(1)
///     .build()
///     .start()
///     .unwrap();
///
/// // Packets arrive in receive order — live and retransmitted are interleaved.
/// // Only minimal validation is performed: datagrams shorter than 20 bytes
/// // are discarded.
/// while let Ok(datagram) = rx.recv() {
///     let packet = Packet::ref_from_bytes(datagram.bytes()).unwrap();
///
///     match packet.packet_kind() {
///         PacketKind::Heartbeat | PacketKind::EndOfSession => continue,
///         PacketKind::Standard => {}
///     }
///
///     // Optionally send additional manual re-requests.
///     if packet.iter().len() != packet.msg_count() as usize {
///         let rereq = RetransmissionPacket {
///             session: *packet.session_ident_raw(),
///             seq_num: packet.seq_num().into(),
///             msg_count: packet.msg_count().into(),
///         };
///         req_tx.try_send(RetransmissionRequest::new(rereq)).unwrap();
///     }
///
///     for msg in packet.iter() {
///         handle(msg.data());
///     }
/// }
///
/// fn handle(_data: &[u8]) {}
/// ```
///
/// # Errors
///
/// [`start`](Self::start) returns an [`io::Error`] if the multicast socket
/// cannot be bound, if joining the multicast group fails, or if the unicast
/// re-request socket cannot be bound. Per-packet send/receive failures on
/// worker threads are logged via [`tracing`] and do not terminate the client.
#[derive(Builder)]
pub struct MoldUDP64 {
    /// Multicast group + port carrying the downstream data stream.
    multicast_addr: SocketAddrV4,
    /// Local interface used to join the multicast group.
    /// Use `Ipv4Addr::UNSPECIFIED` (0.0.0.0) for "any interface".
    interface_addr: Ipv4Addr,
    /// Re-request server(s). Requests are load-balanced across all entries
    /// via an MPMC channel; retransmitted packets are merged back into the
    /// same data channel as live packets.
    rerequest_server_addrs: Vec<SocketAddr>,
    /// If set, only packets matching this session ident are forwarded.
    /// If `None`, the client locks onto the first session it observes.
    expected_session_ident: Option<String>,
    /// First sequence number the consumer cares about. Gaps before this
    /// point are not re-requested. Defaults to the beginning of the session
    /// if omitted.
    expected_seq_num: Option<u64>,
    /// Maximum number of send failures tolerated for a single re-request
    /// before it is abandoned. Default: `100`.
    #[builder(default = 100)]
    max_rerequest_retries: u8,
}

impl MoldUDP64 {
    /// Starts the client, spawning `2 + N` threads where *N* is the number
    /// of configured `rerequest_server_addrs`.
    ///
    /// Returns a pair of:
    ///
    /// - `Receiver<Datagram>` — data channel. Both live and retransmitted
    ///   packets arrive here in receive order.
    /// - `Sender<RetransmissionRequest>` — re-request channel. Use this to
    ///   manually inject additional gap-fill requests alongside the automatic
    ///   gap detection.
    ///
    /// # Errors
    ///
    /// Returns an error if any socket cannot be created or configured.
    pub fn start(&self) -> io::Result<(Receiver<Datagram>, Sender<RetransmissionRequest>)> {
        let mcast_socket = UdpSocket::bind(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            self.multicast_addr.port(),
        ))?;
        mcast_socket.join_multicast_v4(self.multicast_addr.ip(), &self.interface_addr)?;
        let rereq_socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        info!(
            multicast_addr = %self.multicast_addr,
            interface = %self.interface_addr,
            rereq_local = ?rereq_socket.local_addr().ok(),
            "joined multicast group"
        );

        self.start_with_sockets(mcast_socket, rereq_socket, &self.rerequest_server_addrs)
    }

    /// Test seam — accepts pre-bound sockets so tests can drive the client
    /// over loopback unicast without needing multicast kernel support.
    pub fn start_with_sockets(
        &self,
        downstream: UdpSocket,
        rereq: UdpSocket,
        servers: &[SocketAddr],
    ) -> io::Result<(Receiver<Datagram>, Sender<RetransmissionRequest>)> {
        info!(
            servers = ?servers,
            expected_session = ?self.expected_session_ident,
            expected_seq_num = ?self.expected_seq_num,
            max_rerequest_retries = self.max_rerequest_retries,
            pool_size = POOL_SIZE,
            buf_size = BUF_SIZE,
            "starting MoldUDP64 client"
        );

        // --- Buffer pool ---
        let pool: Pool = Arc::new(ArrayQueue::new(POOL_SIZE));
        for _ in 0..POOL_SIZE {
            let _ = pool.push(vec![0u8; BUF_SIZE].into_boxed_slice());
        }

        // --- Channels ---
        let (data_tx, data_rx) = channel::bounded::<Datagram>(POOL_SIZE);
        let (req_tx, req_rx) = channel::bounded::<RetransmissionRequest>(POOL_SIZE);

        // Thread — multicast receiver, drives gap detection.
        {
            let pool = Arc::clone(&pool);
            let data_tx = data_tx.clone();
            let mut session = self.expected_session_ident.as_ref().map(|s| pad_session(s));
            let mut seq = self.expected_seq_num;
            let req_tx = req_tx.clone();
            spawn(move || {
                let span = info_span!("multicast_recv");
                let _enter = span.enter();
                multicast_recv_loop(downstream, pool, data_tx, req_tx, &mut session, &mut seq);
            });
        }

        let rereq_socket = Arc::new(rereq);
        // Threads — one re-request sender per server, all draining the same channel.
        // crossbeam channels are MPMC, so whichever sender is idle grabs the next
        // Request. This spreads load across servers and tolerates a slow peer
        // without head-of-line blocking.
        for &server_addr in servers {
            let socket = Arc::clone(&rereq_socket);
            let req_rx = req_rx.clone();
            let req_tx = req_tx.clone();
            let max_rerequest_retries = self.max_rerequest_retries;
            spawn(move || {
                let span = info_span!("rereq_send", server = %server_addr);
                let _enter = span.enter();
                debug!("re-request sender thread started");
                while let Ok(RetransmissionRequest { req, attempts }) = req_rx.recv() {
                    let seq = req.seq_num.get();
                    let msg_count = req.msg_count.get();
                    if attempts >= max_rerequest_retries {
                        warn!(
                            seq,
                            msg_count, attempts, "abandoning re-request: max retries reached"
                        );
                        continue;
                    }
                    match socket.send_to(req.as_bytes(), server_addr) {
                        Ok(_) => trace!(seq, msg_count, attempts, "sent re-request"),
                        Err(error) => {
                            warn!(
                                error = %error,
                                seq,
                                msg_count,
                                attempts,
                                "re-request send failed; requeuing"
                            );
                            if req_tx
                                .try_send(RetransmissionRequest {
                                    attempts: attempts + 1,
                                    req,
                                })
                                .is_err()
                            {
                                error!(
                                    seq,
                                    msg_count, "re-request queue full or disconnected; dropping"
                                );
                            }
                        }
                    }
                }
                debug!("re-request sender thread exiting");
            });
        }
        drop(req_rx); // last clone lives in the spawned threads

        // Thread — re-request response receiver. All servers reply to the same
        // local socket, so a single reader merges retransmissions back into the
        // data channel.
        {
            let pool = Arc::clone(&pool);
            let data_tx = data_tx.clone();
            let socket = Arc::clone(&rereq_socket);
            spawn(move || {
                let span = info_span!("rereq_recv");
                let _enter = span.enter();
                rerequest_recv_loop(socket, pool, data_tx);
            });
        }

        Ok((data_rx, req_tx))
    }
}

/// Outcome of processing a freshly-received datagram.
///
/// Lets the recv loops share the post-recv handling logic with the io_uring
/// backend without each having to duplicate parse + gap-detection + forward.
pub(crate) enum LoopAction {
    /// Continue draining datagrams.
    Continue,
    /// Stop the loop (typically: a packet failed to parse as a `Packet` DST,
    /// which should never happen with our `BUF_SIZE`, but is treated as a
    /// signal to bail out).
    Stop,
}

#[cfg(not(all(feature = "iouring", target_os = "linux")))]
fn multicast_recv_loop(
    socket: UdpSocket,
    pool: Pool,
    data_tx: Sender<Datagram>,
    req_tx: Sender<RetransmissionRequest>,
    expected_session_ident: &mut Option<[u8; 10]>,
    expected_seq_num: &mut Option<u64>,
) {
    debug!("multicast recv loop started");
    loop {
        let mut buf = pool.pop().unwrap_or_else(|| {
            trace!("buffer pool exhausted; falling back to heap allocation");
            vec![0u8; BUF_SIZE].into_boxed_slice()
        });

        let n = match socket.recv(&mut buf[..]) {
            Ok(n) => n,
            Err(error) => {
                error!(error = %error, "multicast recv failed; loop exiting");
                let _ = pool.push(buf);
                break;
            }
        };

        match process_multicast_datagram(
            buf,
            n,
            &pool,
            &data_tx,
            &req_tx,
            expected_session_ident,
            expected_seq_num,
        ) {
            LoopAction::Continue => {}
            LoopAction::Stop => break,
        }
    }
    debug!("multicast recv loop exited");
}

#[cfg(not(all(feature = "iouring", target_os = "linux")))]
fn rerequest_recv_loop(socket: Arc<UdpSocket>, pool: Pool, data_tx: Sender<Datagram>) {
    debug!("re-request recv loop started");
    loop {
        let mut buf = pool.pop().unwrap_or_else(|| {
            trace!("buffer pool exhausted; falling back to heap allocation");
            vec![0u8; BUF_SIZE].into_boxed_slice()
        });

        let n = match socket.recv(&mut buf[..]) {
            Ok(n) => n,
            Err(error) => {
                error!(error = %error, "re-request recv failed; loop exiting");
                let _ = pool.push(buf);
                break;
            }
        };

        match process_rereq_datagram(buf, n, &pool, &data_tx) {
            LoopAction::Continue => {}
            LoopAction::Stop => break,
        }
    }
    debug!("re-request recv loop exited");
}

#[cfg(all(feature = "iouring", target_os = "linux"))]
use crate::iouring::{multicast_recv_loop, rerequest_recv_loop};

/// Parses a multicast datagram, runs gap-detection, and forwards it to the
/// consumer. Returns the buffer to the pool on the discard paths.
pub(crate) fn process_multicast_datagram(
    buf: Buffer,
    n: usize,
    pool: &Pool,
    data_tx: &Sender<Datagram>,
    req_tx: &Sender<RetransmissionRequest>,
    expected_session_ident: &mut Option<[u8; 10]>,
    expected_seq_num: &mut Option<u64>,
) -> LoopAction {
    if n < Packet::MIN_PACKET_LEN {
        warn!(bytes = n, "discarding short multicast datagram");
        let _ = pool.push(buf);
        return LoopAction::Continue;
    }

    let packet = match Packet::ref_from_bytes(&buf) {
        Ok(packet) => packet,
        Err(error) => {
            error!(error = %error, bytes = n, "multicast packet parse failed; loop exiting");
            let _ = pool.push(buf);
            return LoopAction::Stop;
        }
    };

    let pkt_session = *packet.session_ident_raw();
    let pkt_seq = packet.seq_num();
    let pkt_msg_count = packet.msg_count();
    trace!(
        session = %String::from_utf8_lossy(&pkt_session),
        seq = pkt_seq,
        msg_count = pkt_msg_count,
        bytes = n,
        "received multicast packet"
    );

    // Gap detection: if the live stream has skipped ahead of what we were
    // expecting, ask the re-request server for the missing range.
    match (expected_session_ident.deref(), *expected_seq_num) {
        (Some(exp_session), Some(exp_seq)) if *exp_session == pkt_session => {
            if pkt_seq > exp_seq {
                let gap = pkt_seq - exp_seq;
                let msg_count = gap.min(u16::MAX as u64) as u16;
                warn!(
                    session = %String::from_utf8_lossy(&pkt_session),
                    expected_seq = exp_seq,
                    received_seq = pkt_seq,
                    gap,
                    rereq_msg_count = msg_count,
                    "sequence gap detected; enqueuing re-request"
                );
                let req = RetransmissionPacket {
                    session: pkt_session,
                    seq_num: exp_seq.into(),
                    msg_count: msg_count.into(),
                };
                if req_tx.try_send(RetransmissionRequest::new(req)).is_err() {
                    error!(
                        seq = exp_seq,
                        msg_count, "re-request queue full or disconnected"
                    );
                }
            }
        }
        (Some(exp_session), _) => {
            // Session change: we have no idea what to ask for; just resync.
            info!(
                prev_session = %String::from_utf8_lossy(exp_session),
                new_session = %String::from_utf8_lossy(&pkt_session),
                new_seq = pkt_seq,
                "session change detected; resynchronising"
            );
        }
        (None, _) => {
            info!(
                session = %String::from_utf8_lossy(&pkt_session),
                seq = pkt_seq,
                "locked onto session"
            );
        }
    }

    // Advance expectation to the seq right after this packet's last msg.
    *expected_seq_num = Some(pkt_seq + pkt_msg_count as u64);
    *expected_session_ident = Some(pkt_session);
    forward(data_tx, pool, buf, n, "multicast");
    LoopAction::Continue
}

/// Forwards a retransmission datagram to the consumer without touching the
/// gap-detection state — retransmissions are historic and out-of-order.
pub(crate) fn process_rereq_datagram(
    buf: Buffer,
    n: usize,
    pool: &Pool,
    data_tx: &Sender<Datagram>,
) -> LoopAction {
    if n < Packet::MIN_PACKET_LEN {
        warn!(bytes = n, "discarding short retransmission datagram");
        let _ = pool.push(buf);
        return LoopAction::Continue;
    }

    if let Ok(packet) = Packet::ref_from_bytes(&buf[..]) {
        trace!(
            session = %String::from_utf8_lossy(packet.session_ident_raw()),
            seq = packet.seq_num(),
            msg_count = packet.msg_count(),
            bytes = n,
            "received retransmission"
        );
    }

    // Retransmissions are out-of-order historic packets — we deliberately
    // do NOT advance expected_* state here. The consumer reorders by
    // (session_ident, seq_num).
    forward(data_tx, pool, buf, n, "retx");
    LoopAction::Continue
}

#[inline]
pub(crate) fn forward(
    data_tx: &Sender<Datagram>,
    pool: &Pool,
    buf: Buffer,
    len: usize,
    source: &'static str,
) {
    let dgram = Datagram {
        buf: Some(buf),
        len,
        pool: Arc::clone(pool),
    };
    match data_tx.try_send(dgram) {
        Err(channel::TrySendError::Full(_)) => {
            warn!(source, len, "dropping datagram: consumer channel full")
        }
        Err(channel::TrySendError::Disconnected(_)) => {
            warn!(source, "dropping datagram: consumer channel disconnected");
        }
        Ok(()) => {}
    }
}

/// Size of buffer when reading from socket. Spec max is 64 KiB; oversized
/// here for headroom against any future framing quirks.
pub(crate) const BUF_SIZE: usize = 524_288;
const POOL_SIZE: usize = 1024;

pub(crate) type Buffer = Box<[u8]>;
pub(crate) type Pool = Arc<ArrayQueue<Buffer>>;

/// A received UDP datagram backed by a pooled buffer.
///
/// `Datagram` holds a lease on one of the client's pre-allocated 512 KiB
/// receive buffers. Dropping it returns the buffer to the pool automatically.
/// Do not hold `Datagram` values longer than necessary; a saturated pool
/// causes the client to fall back to heap allocation.
///
/// Obtain a zero-copy [`Packet`] view via:
///
/// ```no_run
/// use moldudp::{Datagram, FromBytes, Packet};
///
/// fn inspect(datagram: &Datagram) {
///     let packet = Packet::ref_from_bytes(datagram.bytes()).unwrap();
///     println!("seq={}", packet.seq_num());
/// }
/// ```
pub struct Datagram {
    buf: Option<Buffer>,
    len: usize,
    pool: Pool,
}

impl Datagram {
    /// Returns the received bytes (header + message blocks).
    ///
    /// The slice is valid for the lifetime of this `Datagram`; it is backed
    /// by a pooled buffer that is returned on drop.
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.buf.as_ref().unwrap()[..self.len]
    }
}

impl Drop for Datagram {
    fn drop(&mut self) {
        if let Some(buf) = self.buf.take() {
            let _ = self.pool.push(buf);
        }
    }
}

/// A re-request sent to a re-request server to fill a sequence gap.
///
/// Wraps a [`RetransmissionPacket`] (which is just a [`PacketHeader`] filled
/// with the start of the missing range and the number of messages wanted) and
/// tracks how many send attempts have been made.
///
/// Construct with [`RetransmissionRequest::new`] and send via the
/// `Sender<RetransmissionRequest>` returned by [`MoldUDP64::start`].
pub struct RetransmissionRequest {
    req: RetransmissionPacket,
    attempts: u8,
}

impl RetransmissionRequest {
    /// Creates a new re-request with zero prior attempts.
    #[inline]
    pub const fn new(packet: RetransmissionPacket) -> Self {
        RetransmissionRequest {
            req: packet,
            attempts: 0,
        }
    }
}

/// The packet sent to a re-request server to ask for retransmission of a
/// range of messages.
///
/// Encoded as a standard 20-byte MoldUDP64 header:
///
/// - `session` — the session the missing messages belong to.
/// - `seq_num` — sequence number of the first wanted message.
/// - `msg_count` — how many consecutive messages are wanted (max `u16::MAX`).
///
/// The server responds with one or more standard downstream packets unicast
/// back to the sender. Clients may process retransmissions on the same socket
/// used for multicast so only one receive socket is needed.
pub type RetransmissionPacket = PacketHeader;

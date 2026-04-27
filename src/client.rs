use crossbeam::channel::{self, Receiver, Sender};
use crossbeam::queue::ArrayQueue;
use std::{
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    sync::Arc,
    thread::spawn,
};
use tracing::{error, warn};

use crate::packet::Packet;
use bon::Builder;

/// MoldUDP64 client.
///
/// Subscribes to the downstream multicast group and transparently re-requests
/// any missed packets from a unicast re-request server. Both live and
/// retransmitted Downstream packets are surfaced through a single channel of
/// [`Datagram`]s — the consumer reassembles by session ident + seq num.
///
/// # Example
///
/// ```no_run
/// use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
/// use moldudp::{MoldUDP64,Packet};
///
/// let rx = MoldUDP64::builder()
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
///     // Optional: pin the expected session. If set, packets from any other
///     // session ident are dropped. If omitted, the client locks onto the
///     // first session it sees.
///     .expected_session_ident("0123456789".to_string())
///     // Optional: starting sequence number. Defaults to 1 (the start of
///     // the session) if omitted; gaps before this point are not requested.
///     .expected_seq_num(1)
///     .build()
///     .start()?;
///
/// // Datagrams arrive in receive order — live and retransmitted packets are
/// // interleaved. The consumer is responsible for ordering by seq num.
/// // Minimal validation is done on datagrams, only that they are at least
/// // 20 bytes in length.
/// while let Ok(datagram) = rx.recv() {
///     // Use [`moldudp::Packet`] to construct a 0 allocation view on the bytes.
///     let packet = Packet::new(packet);
///     handle(packet);
/// }
/// ```
///
/// # Errors
///
/// [`start`](Self::start) returns an [`io::Error`] if the multicast socket
/// cannot be bound, if joining the multicast group fails, or if the unicast
/// re-request socket cannot be bound. Per-packet send/receive failures on
/// the worker threads are logged and do not terminate the client.
#[derive(Builder)]
pub struct MoldUDP64 {
    /// Multicast group + port carrying the downstream data stream.
    multicast_addr: SocketAddrV4,
    /// Local interface used to join the multicast group.
    /// Use `Ipv4Addr::UNSPECIFIED` (0.0.0.0) for "any".
    interface_addr: Ipv4Addr,
    /// Re-request server(s). Requests are load-balanced across all entries;
    /// retransmitted Downstream packets come back on the same unicast socket.
    rerequest_server_addrs: Vec<SocketAddr>,
    /// If set, only packets matching this session ident are accepted.
    /// If `None`, the client locks onto the first session it observes.
    expected_session_ident: Option<String>,
    /// First sequence number the consumer cares about. Gaps before this
    /// point are not re-requested.
    expected_seq_num: Option<u64>,
    /// Max number of failures tolerated for a re-request server before shutting it down.
    #[builder(default = 100)]
    max_rerequest_retries: u64,
}

impl MoldUDP64 {
    #[must_use]
    /// Starts the client with 2 + N [`rerequest_server_addrs`](Self::rerequest_server_addrs) threads.
    /// Dumps received datagrams into returned receiver channel.
    /// Datagrams arrive in receive order — live and retransmitted packets are
    /// interleaved. The consumer is responsible for ordering by seq num.
    /// Minimal validation is done on datagrams, only that they are at least
    /// 20 bytes in length.
    pub fn start(&self) -> io::Result<Receiver<Datagram>> {
        let mcast_socket = UdpSocket::bind(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            self.multicast_addr.port(),
        ))?;
        mcast_socket.join_multicast_v4(self.multicast_addr.ip(), &self.interface_addr)?;
        let rereq_socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;

        self.start_with_sockets(mcast_socket, rereq_socket, &self.rerequest_server_addrs)
    }

    /// Test seam — accepts pre-bound sockets so tests can drive the client
    /// over loopback unicast without needing multicast support.
    pub(crate) fn start_with_sockets(
        &self,
        downstream: UdpSocket,
        rereq: UdpSocket,
        servers: &[SocketAddr],
    ) -> io::Result<Receiver<Datagram>> {
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
            let mut session = self.expected_session_ident.clone();
            let mut seq = self.expected_seq_num;
            let req_tx = req_tx.clone();
            spawn(move || {
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
                let mut buf = [0u8; 20];
                let mut n_failures = 0;
                while let Ok(req) = req_rx.recv()
                    && n_failures < max_rerequest_retries
                {
                    req.serialize_into(&mut buf);
                    if let Err(e) = socket.send_to(buf.as_slice(), &server_addr) {
                        warn!("failed to send re-request to {server_addr}: {e}");
                        n_failures += 1;
                        if req_tx.try_send(req).is_err() {
                            error!("re-request queue full or disconnected");
                        }
                    }
                }
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
            spawn(move || rerequest_recv_loop(socket, pool, data_tx));
        }

        Ok(data_rx)
    }
}

fn multicast_recv_loop(
    socket: UdpSocket,
    pool: Pool,
    data_tx: Sender<Datagram>,
    req_tx: Sender<RetransmissionRequest>,
    expected_session_ident: &mut Option<String>,
    expected_seq_num: &mut Option<u64>,
) {
    loop {
        let mut buf = pool
            .pop()
            .unwrap_or_else(|| vec![0u8; BUF_SIZE].into_boxed_slice());

        let n = match socket.recv(&mut buf[..]) {
            Ok(n) => n,
            Err(e) => {
                error!("multicast recv error: {e}");
                let _ = pool.push(buf);
                break;
            }
        };

        if n < Packet::MIN_PACKET_LEN {
            error!("incomplete multicast datagram");
            let _ = pool.push(buf);
            continue;
        }

        let packet = Packet::new(&buf);

        // Gap detection: if the live stream has skipped ahead of what we were
        // expecting, ask the re-request server for the missing range.
        if let (Some(exp_session), Some(exp_seq)) =
            (expected_session_ident.as_deref(), *expected_seq_num)
        {
            let session_matches = exp_session == packet.session_ident();
            if session_matches && packet.seq_num() > exp_seq {
                let gap = packet.seq_num() - exp_seq;
                let msg_count = gap.min(u16::MAX as u64) as u16;
                let req = RetransmissionRequest {
                    session: *packet.session_ident_raw(),
                    seq_num: exp_seq,
                    msg_count: msg_count,
                };
                if req_tx.try_send(req).is_err() {
                    error!("re-request queue full or disconnected");
                }
            }
            // Session change: we have no idea what to ask for; just resync.
        }

        // Advance expectation to the seq right after this packet's last msg.
        *expected_seq_num = Some(packet.seq_num() + packet.msg_count() as u64);
        match expected_session_ident {
            Some(s) => {
                s.clear();
                s.push_str(packet.session_ident());
            }
            None => *expected_session_ident = Some(packet.session_ident().to_owned()),
        }

        forward(&data_tx, &pool, buf, n, "multicast");
    }
}

fn rerequest_recv_loop(socket: Arc<UdpSocket>, pool: Pool, data_tx: Sender<Datagram>) {
    loop {
        let mut buf = pool
            .pop()
            .unwrap_or_else(|| vec![0u8; BUF_SIZE].into_boxed_slice());

        let n = match socket.recv(&mut buf[..]) {
            Ok(n) => n,
            Err(e) => {
                error!("re-request recv error: {e}");
                let _ = pool.push(buf);
                break;
            }
        };

        if n < Packet::MIN_PACKET_LEN {
            error!("incomplete retransmission datagram");
            let _ = pool.push(buf);
            continue;
        }

        // Retransmissions are out-of-order historic packets — we deliberately
        // do NOT advance expected_* state here. The consumer reorders by
        // (session_ident, seq_num).
        forward(&data_tx, &pool, buf, n, "retx");
    }
}

#[inline]
fn forward(data_tx: &Sender<Datagram>, pool: &Pool, buf: Buffer, len: usize, src: &'static str) {
    let dgram = Datagram {
        buf: Some(buf),
        len,
        pool: Arc::clone(pool),
    };
    match data_tx.try_send(dgram) {
        Err(channel::TrySendError::Full(_)) => warn!("datagram consumer full ({src})"),
        Err(channel::TrySendError::Disconnected(_)) => {
            warn!("datagram consumer dropped ({src})");
        }
        Ok(()) => {}
    }
}

/// Size of buffer when reading from socket. Spec max is 64 KiB; oversized
/// here for headroom against any future framing quirks.
const BUF_SIZE: usize = 524_288;
const POOL_SIZE: usize = 1024;

type Buffer = Box<[u8]>;
type Pool = Arc<ArrayQueue<Buffer>>;

pub struct Datagram {
    buf: Option<Buffer>,
    len: usize,
    pool: Pool,
}

impl Datagram {
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

/// The Request Packet is sent to request the retransmission of a particular message or group of messages. The
/// request packet is sent to a Re-request server. A receiver may need to send this request when it detects a
/// sequence number gap in received messages. The response to a valid Request Packet is a standard Downstream
/// Packet unicast back to the source of the retransmission request. This allows downstream MoldUDP64 users to
/// read the retransmitted Downstream Packet in their multicast processing socket if the request was made from
/// that socket (in other words, the client need only have one socket open to listen to the multicast and to process
/// retransmissions, even though the retransmissions are not multicast).

struct RetransmissionRequest {
    session: [u8; 10],
    seq_num: u64,
    msg_count: u16,
}
impl RetransmissionRequest {
    #[inline]
    fn serialize_into(&self, buf: &mut [u8; 20]) {
        buf[Self::SESSION_OFFSET..Self::SESSION_LENGTH].copy_from_slice(&self.session);

        let end = Self::SEQ_OFFSET + Self::SEQ_LENGTH;
        buf[Self::SEQ_OFFSET..end].copy_from_slice(&self.seq_num.to_be_bytes());

        let end = Self::MSG_COUNT_OFFSET + Self::MSG_COUNT_LENGTH;
        buf[Self::MSG_COUNT_OFFSET..end].copy_from_slice(&self.msg_count.to_be_bytes());
    }

    const SESSION_OFFSET: usize = 0;
    const SESSION_LENGTH: usize = 10;

    const SEQ_OFFSET: usize = 10;
    const SEQ_LENGTH: usize = 8;

    const MSG_COUNT_OFFSET: usize = 18;
    const MSG_COUNT_LENGTH: usize = 2;
}

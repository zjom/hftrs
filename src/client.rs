use crossbeam::channel::{self, Receiver, Sender};
use crossbeam::queue::ArrayQueue;
use std::{
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    sync::Arc,
    thread::spawn,
};
use tracing::{error, warn};

use crate::{Request, packet::DownstreamPacket};

/// MoldUDP64 client.
///
/// Subscribes to the downstream multicast group and transparently re-requests
/// any missed packets from a unicast re-request server. Both live and
/// retransmitted Downstream packets are surfaced through a single channel of
/// [`PooledDatagram`]s — the consumer reassembles by session ident + seq num.
pub struct MoldUDP64 {
    /// Multicast group + port carrying the downstream data stream.
    pub multicast_addr: SocketAddrV4,
    /// Local interface used to join the multicast group.
    /// Use `Ipv4Addr::UNSPECIFIED` (0.0.0.0) for "any".
    pub interface_addr: Ipv4Addr,
    /// Re-request server(s). Requests are sent to the first entry; the
    /// retransmitted Downstream packets come back on the same unicast socket.
    /// TODO: Extend to round-robin / failover
    pub rerequest_server_addrs: Vec<SocketAddr>,
    pub expected_session_ident: Option<String>,
    pub expected_seq_num: Option<u64>,
}

impl MoldUDP64 {
    pub fn start(&self) -> io::Result<Receiver<PooledDatagram>> {
        // --- Downstream multicast socket (recv-only) ---
        let mcast_socket = UdpSocket::bind(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            self.multicast_addr.port(),
        ))?;
        mcast_socket.join_multicast_v4(self.multicast_addr.ip(), &self.interface_addr)?;

        // --- Re-request socket (shared unicast: all senders + the response receiver) ---
        if self.rerequest_server_addrs.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "at least one re-request server address is required",
            ));
        }
        let rereq_socket = Arc::new(UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?);

        // --- Buffer pool ---
        let pool: Pool = Arc::new(ArrayQueue::new(POOL_SIZE));
        for _ in 0..POOL_SIZE {
            let _ = pool.push(vec![0u8; BUF_SIZE].into_boxed_slice());
        }

        // --- Channels ---
        let (data_tx, data_rx) = channel::bounded::<PooledDatagram>(POOL_SIZE);
        let (req_tx, req_rx) = channel::bounded::<Request>(POOL_SIZE);

        // Thread — multicast receiver, drives gap detection.
        {
            let pool = Arc::clone(&pool);
            let data_tx = data_tx.clone();
            let mut session = self.expected_session_ident.clone();
            let mut seq = self.expected_seq_num;
            spawn(move || {
                multicast_recv_loop(mcast_socket, pool, data_tx, req_tx, &mut session, &mut seq);
            });
        }

        // Threads — one re-request sender per server, all draining the same channel.
        // crossbeam channels are MPMC, so whichever sender is idle grabs the next
        // Request. This spreads load across servers and tolerates a slow peer
        // without head-of-line blocking.
        for &server_addr in &self.rerequest_server_addrs {
            let socket = Arc::clone(&rereq_socket);
            let req_rx = req_rx.clone();
            spawn(move || {
                while let Ok(req) = req_rx.recv() {
                    if let Err(e) = socket.send_to(req.as_bytes(), &server_addr) {
                        error!("failed to send re-request to {server_addr}: {e}");
                        // Optional: requeue with `req_tx.try_send(req)` so another
                        // server picks it up. Be careful about infinite loops if
                        // every server is down.
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
    data_tx: Sender<PooledDatagram>,
    req_tx: Sender<Request>,
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

        if n < DownstreamPacket::MIN_PACKET_LEN {
            error!("incomplete multicast datagram");
            let _ = pool.push(buf);
            continue;
        }

        let packet = DownstreamPacket::new(&buf);

        // Gap detection: if the live stream has skipped ahead of what we were
        // expecting, ask the re-request server for the missing range.
        if let (Some(exp_session), Some(exp_seq)) =
            (expected_session_ident.as_deref(), *expected_seq_num)
        {
            let session_matches = exp_session == packet.session_ident();
            if session_matches && packet.seq_num() > exp_seq {
                let gap = packet.seq_num() - exp_seq;
                let msg_count = gap.min(u16::MAX as u64) as u16;
                let req = Request::new(exp_session, exp_seq, msg_count);
                if req_tx.try_send(req).is_err() {
                    error!("re-request queue full or disconnected");
                }
            }
            // Session change: we have no idea what to ask for; just resync.
        }

        // Advance expectation to the seq right after this packet's last msg.
        *expected_seq_num = Some(packet.seq_num() + packet.msg_count() as u64);
        *expected_session_ident = Some(packet.session_ident().to_string());

        forward(&data_tx, &pool, buf, n, "multicast");
    }
}

fn rerequest_recv_loop(socket: Arc<UdpSocket>, pool: Pool, data_tx: Sender<PooledDatagram>) {
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

        if n < DownstreamPacket::MIN_PACKET_LEN {
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
fn forward(
    data_tx: &Sender<PooledDatagram>,
    pool: &Pool,
    buf: Buffer,
    len: usize,
    src: &'static str,
) {
    let dgram = PooledDatagram {
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

pub struct PooledDatagram {
    buf: Option<Buffer>,
    len: usize,
    pool: Pool,
}

impl PooledDatagram {
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.buf.as_ref().unwrap()[..self.len]
    }
}

impl Drop for PooledDatagram {
    fn drop(&mut self) {
        if let Some(buf) = self.buf.take() {
            let _ = self.pool.push(buf);
        }
    }
}

//! Test server for MoldUDP64 client development and testing.
//!
//! Broadcasts MoldUDP64 packets on one socket and answers re-request traffic
//! on another. Intended for unit tests, integration tests, and local
//! experimentation — not for production deployment. The server stores every
//! individual message in an in-memory log keyed by sequence number, so it
//! can synthesise a response to any range the client asks for without caring
//! about original packet boundaries.
//!
//! Heartbeats are emitted automatically at `heartbeat_interval` (default: 1 s).
//! After [`ServerHandle::shutdown`] the periodic packet switches from a
//! heartbeat to an end-of-session, and [`ServerHandle::send`] /
//! [`ServerHandle::send_dropped`] are refused. The re-request thread keeps
//! running until the server is dropped.
//!
//! # Wiring against the client
//!
//! ```ignore
//! let server = MoldUDP64Server::builder()
//!     .multicast_addr("239.1.2.3:5000".parse().unwrap())
//!     .rerequest_bind_addr("127.0.0.1:6000".parse().unwrap())
//!     .session("TESTSESSN".to_string())
//!     .build();
//! let h = server.start()?;
//!
//! h.send(vec![b"hello".to_vec()]);            // seq 1 — live broadcast
//! h.send_dropped(vec![b"missing".to_vec()]);  // seq 2 — stored only; client must re-request
//! h.send(vec![b"world".to_vec()]);            // seq 3 — triggers gap detection
//! // Heartbeats are sent automatically every second.
//! h.shutdown();                               // End-of-session replaces heartbeats.
//! ```

use std::collections::BTreeMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread::spawn;
use std::time::Duration;

use bon::Builder;
use crossbeam::channel::{self, Receiver, RecvTimeoutError, Sender};
use tracing::{debug, error, info, info_span, trace, warn};

use crate::packet;
use crate::util::pad_session;

const HEADER_LEN: usize = 20;

type DB = Mutex<BTreeMap<u64, Vec<u8>>>;

/// Commands processed by the running sender thread.
///
/// Sent via [`ServerHandle`]; not normally constructed directly.
#[derive(Debug, Clone)]
pub enum ServerCommand {
    /// Build and broadcast a packet containing these messages, and store them
    /// in the retransmission log. Advances the sequence number by the number
    /// of messages.
    Send(Vec<Vec<u8>>),
    /// Store messages in the retransmission log *without* broadcasting them.
    /// The next [`Send`](Self::Send) will leave a gap in the live stream that
    /// the client must fill via a re-request.
    SendDropped(Vec<Vec<u8>>),
    /// Immediately broadcast a heartbeat (`msg_count = 0`). Heartbeats are
    /// also emitted automatically on each `heartbeat_interval` tick; this is
    /// for tests that need one at a specific moment. Does not advance the
    /// sequence number. After [`StopSession`](Self::StopSession) this sends an
    /// end-of-session instead, matching the periodic behaviour.
    Heartbeat,
    /// Immediately broadcast an end-of-session packet (`msg_count = 0xFFFF`).
    /// End-of-session packets are sent automatically in place of heartbeats
    /// after [`StopSession`](Self::StopSession).
    EndOfSession,
    /// Transition the session to the stopped state. From this point:
    ///
    /// - Periodic heartbeats are replaced with end-of-session packets.
    /// - [`Send`](Self::Send) and [`SendDropped`](Self::SendDropped) are
    ///   refused (logged as warnings).
    /// - The re-request thread continues to serve retransmission requests.
    StopSession,
    /// Change the session identifier used on outgoing packets. Does not send
    /// an end-of-session for the previous session; call
    /// [`StopSession`](Self::StopSession) first if clients need to know.
    ChangeSession(String),
}

/// Builder-configured MoldUDP64 test server.
///
/// Use [`MoldUDP64Server::builder()`] to construct, then call [`start`] to
/// spawn the background threads and obtain a [`ServerHandle`].
///
/// [`start`]: MoldUDP64Server::start
#[derive(Builder)]
pub struct MoldUDP64Server {
    /// Destination for live broadcast packets. Use a multicast group + port
    /// for realistic tests; for pure loopback tests set this to the client's
    /// downstream bind address.
    multicast_addr: SocketAddrV4,
    /// Outbound interface for multicast sends. Has no effect when
    /// `multicast_addr` is a unicast address.
    #[builder(default = Ipv4Addr::UNSPECIFIED)]
    interface_addr: Ipv4Addr,
    /// Local address the re-request server listens on. The client sends
    /// [`RetransmissionRequest`] datagrams here.
    ///
    /// [`RetransmissionRequest`]: crate::RetransmissionRequest
    rerequest_bind_addr: SocketAddr,
    /// Session identifier for outgoing packets. Strings shorter than 10 bytes
    /// are right-padded with spaces; longer strings are truncated to 10 bytes.
    #[builder(into)]
    session: String,
    /// Maximum UDP payload for outgoing packets.
    /// Default: `1452` (1500 MTU − 20 IP − 8 UDP − 20 MoldUDP64 header).
    /// Messages that individually exceed this limit are placed in their own
    /// packet.
    #[builder(default = 1452)]
    max_payload: usize,
    /// How often the server sends a periodic heartbeat (or end-of-session
    /// after shutdown). Default: 1 second.
    #[builder(default = Duration::from_secs(1), into)]
    heartbeat_interval: Duration,
    /// Capacity of the internal command queue. Default: `1_000_000`.
    /// Set to `0` for an unbounded queue.
    #[builder(default = 1_000_000)]
    command_queue_size: usize,
    /// Sequence number assigned to the very first message. Default: `1`.
    #[builder(default = 1)]
    seq_num: u64,
}

/// Handle to a running [`MoldUDP64Server`].
///
/// All methods send a command to the server's background sender thread.
/// Sending is fire-and-forget: if the command queue is full the call returns
/// silently (an error is logged internally).
///
/// The server sender thread exits when all `ServerHandle` clones are dropped.
pub struct ServerHandle {
    pub tx: Sender<ServerCommand>,
}

impl ServerHandle {
    /// Broadcast `msgs` as a downstream packet and store them in the
    /// retransmission log. Advances the session's sequence number by
    /// `msgs.len()`.
    ///
    /// Has no effect (and logs a warning) if the session has been stopped.
    pub fn send(&self, msgs: Vec<Vec<u8>>) {
        let _ = self.tx.send(ServerCommand::Send(msgs));
    }

    /// Store `msgs` in the retransmission log *without* broadcasting them,
    /// creating a deliberate gap in the live stream. Advances the sequence
    /// number by `msgs.len()`.
    ///
    /// Useful for simulating packet loss so the client's gap-detection and
    /// re-request logic can be exercised.
    ///
    /// Has no effect (and logs a warning) if the session has been stopped.
    pub fn send_dropped(&self, msgs: Vec<Vec<u8>>) {
        let _ = self.tx.send(ServerCommand::SendDropped(msgs));
    }

    /// Force an immediate heartbeat broadcast outside the normal tick interval.
    ///
    /// After [`shutdown`](Self::shutdown), this sends an end-of-session packet
    /// instead, matching the behaviour of the automatic periodic tick.
    pub fn heartbeat(&self) {
        let _ = self.tx.send(ServerCommand::Heartbeat);
    }

    /// Force an immediate end-of-session broadcast.
    ///
    /// This does not stop the session; use [`shutdown`](Self::shutdown) for
    /// that. This method is for tests that need to inject an explicit
    /// end-of-session at a specific moment.
    pub fn end_of_session(&self) {
        let _ = self.tx.send(ServerCommand::EndOfSession);
    }

    /// Change the session identifier for all subsequent outgoing packets.
    ///
    /// Does not emit an end-of-session for the previous session. If clients
    /// need to know the session has ended, call [`shutdown`](Self::shutdown)
    /// before changing the session.
    ///
    /// Has no effect (and logs a warning) if the session has been stopped.
    pub fn change_session(&self, session: String) {
        let _ = self.tx.send(ServerCommand::ChangeSession(session));
    }

    /// Stop the session: periodic heartbeats are replaced with end-of-session
    /// packets and no further messages can be sent.
    ///
    /// The re-request thread continues running so clients can still fill gaps
    /// while the end-of-session window is open.
    pub fn shutdown(&self) {
        let _ = self.tx.send(ServerCommand::StopSession);
    }
}

impl MoldUDP64Server {
    /// Spawn the server's background threads and return a [`ServerHandle`].
    ///
    /// Two threads are started:
    ///
    /// 1. **Sender** — processes commands from the handle, broadcasts
    ///    packets, and emits periodic heartbeats.
    /// 2. **Re-request responder** — listens for retransmission requests and
    ///    unicasts responses synthesised from the in-memory log.
    ///
    /// # Errors
    ///
    /// Returns an error if either socket cannot be created or bound.
    pub fn start(&self) -> io::Result<ServerHandle> {
        let downstream = UdpSocket::bind(SocketAddrV4::new(self.interface_addr, 0))?;
        let rereq = UdpSocket::bind(self.rerequest_bind_addr)?;
        info!(
            multicast_addr = %self.multicast_addr,
            interface = %self.interface_addr,
            rereq_bind = %self.rerequest_bind_addr,
            downstream_local = ?downstream.local_addr().ok(),
            "bound MoldUDP64 server sockets"
        );
        self.start_with_sockets(downstream, rereq)
    }

    /// Test seam — accepts pre-bound sockets so tests can drive the server
    /// over loopback unicast without needing multicast kernel support. The
    /// `multicast_addr` field is still used as the *destination* for sends.
    pub fn start_with_sockets(
        &self,
        downstream: UdpSocket,
        rereq: UdpSocket,
    ) -> io::Result<ServerHandle> {
        let session = pad_session(&self.session);
        let dest = SocketAddr::V4(self.multicast_addr);
        info!(
            session = %String::from_utf8_lossy(&session),
            dest = %dest,
            max_payload = self.max_payload,
            heartbeat_interval = ?self.heartbeat_interval,
            initial_seq = self.seq_num,
            "starting MoldUDP64 server"
        );
        // Per-message log keyed by absolute seq num. We don't preserve packet
        // boundaries — re-requests synthesise a fresh packet from the range.
        let log: Arc<DB> = Arc::new(Mutex::new(BTreeMap::new()));
        let (cmd_tx, cmd_rx) = {
            if self.command_queue_size == 0 {
                channel::unbounded::<ServerCommand>()
            } else {
                channel::bounded::<ServerCommand>(1_000_000)
            }
        };

        {
            let log = Arc::clone(&log);

            let max_payload = self.max_payload;
            let heartbeat_interval = self.heartbeat_interval;
            let seq_num = self.seq_num;
            spawn(move || {
                let span = info_span!("sender", dest = %dest);
                let _enter = span.enter();
                sender_loop(
                    downstream,
                    dest,
                    session,
                    log,
                    cmd_rx,
                    max_payload,
                    heartbeat_interval,
                    seq_num,
                )
            });
        }
        {
            let log = Arc::clone(&log);
            let max_payload = self.max_payload;
            let local = rereq.local_addr().ok();
            spawn(move || {
                let span = info_span!("rereq", local = ?local);
                let _enter = span.enter();
                rerequest_loop(rereq, session, log, max_payload)
            });
        }

        Ok(ServerHandle { tx: cmd_tx })
    }
}

pub fn build_packet(session: &[u8; 10], seq_num: u64, msgs: &[Vec<u8>]) -> Vec<u8> {
    let total: usize = HEADER_LEN + msgs.iter().map(|m| 2 + m.len()).sum::<usize>();
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(session);
    buf.extend_from_slice(&seq_num.to_be_bytes());
    let count = u16::try_from(msgs.len()).expect("too many messages in one packet");
    buf.extend_from_slice(&count.to_be_bytes());
    for m in msgs {
        let len = u16::try_from(m.len()).expect("message too long");
        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(m);
    }
    buf
}

fn build_special(session: &[u8; 10], seq_num: u64, msg_count: u16) -> [u8; HEADER_LEN] {
    let mut buf = [0u8; HEADER_LEN];
    buf[..10].copy_from_slice(session);
    buf[10..18].copy_from_slice(&seq_num.to_be_bytes());
    buf[18..20].copy_from_slice(&msg_count.to_be_bytes());
    buf
}

fn store_messages(log: &DB, start_seq: u64, msgs: &[Vec<u8>]) {
    let mut g = log.lock().unwrap();
    for (i, m) in msgs.iter().enumerate() {
        g.insert(start_seq + i as u64, m.clone());
    }
}

/// Send the periodic special packet — heartbeat normally, end-of-session once
/// the session has been stopped. Always uses the current `next_seq` and never
/// advances it.
fn send_periodic(
    socket: &UdpSocket,
    dest: SocketAddr,
    session: &[u8; 10],
    next_seq: u64,
    stopped: bool,
) {
    let msg_count = if stopped {
        packet::END_OF_SESSION_IDENT
    } else {
        packet::HEARTBEAT_IDENT
    };
    let kind = if stopped {
        "end-of-session"
    } else {
        "heartbeat"
    };
    let pkt = build_special(session, next_seq, msg_count);
    match socket.send_to(&pkt, dest) {
        Ok(_) => trace!(kind, seq = next_seq, "sent periodic packet"),
        Err(error) => error!(error = %error, kind, seq = next_seq, "periodic send failed"),
    }
}

fn sender_loop(
    socket: UdpSocket,
    dest: SocketAddr,
    mut session: [u8; 10],
    log: Arc<DB>,
    cmd_rx: Receiver<ServerCommand>,
    max_payload: usize,
    heartbeat_interval: Duration,
    mut next_seq: u64,
) {
    debug!(
        session = %String::from_utf8_lossy(&session),
        next_seq,
        "sender loop started"
    );
    let mut stopped = false;

    loop {
        match cmd_rx.recv_timeout(heartbeat_interval) {
            Ok(cmd) => match cmd {
                ServerCommand::Send(msgs) => {
                    if stopped {
                        warn!("Send ignored: session has been stopped");
                        continue;
                    }
                    if msgs.is_empty() {
                        warn!("empty Send ignored; heartbeats are automatic");
                        continue;
                    }

                    let count = msgs.len() as u64;
                    let pkt_seq = next_seq;
                    store_messages(&log, pkt_seq, &msgs);
                    next_seq += count;
                    let chunks = chunk_messages(msgs, max_payload);
                    let n_chunks = chunks.len();
                    for msgs in chunks {
                        let pkt = build_packet(&session, pkt_seq, &msgs);
                        match socket.send_to(&pkt, dest) {
                            Ok(bytes) => trace!(
                                seq = pkt_seq,
                                msg_count = msgs.len(),
                                bytes,
                                "sent downstream packet"
                            ),
                            Err(error) => error!(
                                error = %error,
                                seq = pkt_seq,
                                msg_count = msgs.len(),
                                "downstream send failed"
                            ),
                        }
                    }
                    debug!(
                        start_seq = pkt_seq,
                        msg_count = count,
                        chunks = n_chunks,
                        "broadcast Send completed"
                    );
                }
                ServerCommand::SendDropped(msgs) => {
                    if stopped {
                        warn!("SendDropped ignored: session has been stopped");
                        continue;
                    }
                    if msgs.is_empty() {
                        warn!("empty SendDropped ignored");
                        continue;
                    }
                    let count = msgs.len() as u64;
                    let pkt_seq = next_seq;
                    store_messages(&log, pkt_seq, &msgs);
                    next_seq += count;
                    debug!(
                        start_seq = pkt_seq,
                        msg_count = count,
                        "staged messages without broadcasting (gap injected)"
                    );
                }
                ServerCommand::Heartbeat => {
                    // After StopSession, an explicit heartbeat becomes an EoS
                    // so it matches the behaviour of the periodic tick.
                    send_periodic(&socket, dest, &session, next_seq, stopped);
                }
                ServerCommand::EndOfSession => {
                    let pkt = build_special(&session, next_seq, packet::END_OF_SESSION_IDENT);
                    match socket.send_to(&pkt, dest) {
                        Ok(_) => info!(seq = next_seq, "sent end-of-session"),
                        Err(error) => {
                            error!(error = %error, seq = next_seq, "end-of-session send failed")
                        }
                    }
                }
                ServerCommand::ChangeSession(s) => {
                    if stopped {
                        warn!("ChangeSession ignored: session has been stopped");
                        continue;
                    }
                    let new_session = pad_session(&s);
                    info!(
                        prev_session = %String::from_utf8_lossy(&session),
                        new_session = %String::from_utf8_lossy(&new_session),
                        seq = next_seq,
                        "session identifier changed"
                    );
                    session = new_session;
                }
                ServerCommand::StopSession => {
                    if stopped {
                        continue;
                    }
                    info!(
                        seq = next_seq,
                        "stop-session: end-of-session will replace heartbeats"
                    );
                    stopped = true;
                }
            },
            Err(RecvTimeoutError::Timeout) => {
                send_periodic(&socket, dest, &session, next_seq, stopped);
            }
            Err(RecvTimeoutError::Disconnected) => {
                debug!("command channel disconnected; sender loop exiting");
                break;
            }
        }
    }
}

fn rerequest_loop(socket: UdpSocket, session: [u8; 10], log: Arc<DB>, max_payload: usize) {
    debug!(
        session = %String::from_utf8_lossy(&session),
        "re-request loop started"
    );
    let mut buf = [0u8; HEADER_LEN];
    loop {
        let (n, peer) = match socket.recv_from(&mut buf) {
            Ok(x) => x,
            Err(error) => {
                error!(error = %error, "re-request recv failed; loop exiting");
                break;
            }
        };
        if n < HEADER_LEN {
            warn!(peer = %peer, bytes = n, "discarding short re-request");
            continue;
        }
        if buf[..10] != session[..] {
            warn!(
                peer = %peer,
                expected_session = %String::from_utf8_lossy(&session),
                received_session = %String::from_utf8_lossy(&buf[..10]),
                "discarding re-request: session mismatch"
            );
            continue;
        }
        let start_seq = u64::from_be_bytes(buf[10..18].try_into().unwrap());
        let want = u16::from_be_bytes(buf[18..20].try_into().unwrap()) as u64;
        if want == 0 {
            trace!(peer = %peer, start_seq, "ignoring re-request with msg_count=0");
            continue;
        }
        trace!(peer = %peer, start_seq, want, "received re-request");

        let chunks: Vec<Vec<Vec<u8>>> = {
            // Collect contiguous messages from start_seq. If something is missing
            // we still send what we have — the client will re-ask for the rest.
            let log_g = log.lock().unwrap();
            let msgs = (0..want).map_while(|i| log_g.get(&(start_seq + i)).cloned());
            chunk_messages(msgs, max_payload)
        };

        if chunks.is_empty() {
            debug!(
                peer = %peer,
                start_seq,
                want,
                "no messages in log to satisfy re-request"
            );
            continue;
        }

        let total_msgs: usize = chunks.iter().map(Vec::len).sum();
        for msgs in &chunks {
            let pkt = build_packet(&session, start_seq, msgs);
            match socket.send_to(&pkt, peer) {
                Ok(bytes) => trace!(
                    peer = %peer,
                    seq = start_seq,
                    msg_count = msgs.len(),
                    bytes,
                    "sent retransmission packet"
                ),
                Err(error) => error!(
                    error = %error,
                    peer = %peer,
                    seq = start_seq,
                    "retransmission send failed"
                ),
            }
        }
        debug!(
            peer = %peer,
            start_seq,
            requested = want,
            served = total_msgs,
            chunks = chunks.len(),
            "served re-request"
        );
    }
    debug!("re-request loop exited");
}

fn chunk_messages(
    msgs: impl std::iter::IntoIterator<Item = Vec<u8>>,
    chunk_size: usize,
) -> Vec<Vec<Vec<u8>>> {
    let mut chunks: Vec<Vec<Vec<u8>>> = Vec::new();
    let mut current: Vec<Vec<u8>> = Vec::new();
    let mut current_size: usize = 0;

    for msg in msgs {
        let msg_len = msg.len();

        // If a single message exceeds the limit, it goes in its own chunk.
        // Otherwise, flush the current chunk if adding would overflow.
        if !current.is_empty() && current_size + msg_len > chunk_size {
            chunks.push(std::mem::take(&mut current));
            current_size = 0;
        }

        current_size += msg_len;
        current.push(msg);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

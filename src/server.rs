//! Test server for the MoldUDP64 client.
//!
//! Broadcasts MoldUDP64 packets out one socket and answers re-request
//! traffic on another. Intended for tests, fuzzing, and local experimentation
//! — not production. The server keeps every individual message in an
//! in-memory log keyed by sequence number, so it can synthesise a response
//! to any range the client asks for without caring about original packet
//! boundaries.
//!
//! Heartbeats are emitted automatically once per second. After [`ServerHandle::stop_session`]
//! (which sends [`ServerCommand::StopSession`]) the periodic packet switches
//! from a heartbeat to an end-of-session, and `Send` / `SendDropped` are
//! refused — the re-request thread keeps serving retransmissions. The sender
//! thread exits when all `ServerHandle`s are dropped.
//!
//! Typical wiring against the client:
//!
//! ```ignore
//! // Server
//! let server = MoldUDP64Server::builder()
//!     .multicast_addr("239.1.2.3:5000".parse().unwrap())
//!     .rerequest_bind_addr("127.0.0.1:6000".parse().unwrap())
//!     .session("TESTSESSN".to_string())
//!     .build();
//! let h = server.start()?;
//!
//! h.send(vec![b"hello".to_vec()]);            // seq 1
//! h.send_dropped(vec![b"missing".to_vec()]);  // seq 2 -- client will re-request
//! h.send(vec![b"world".to_vec()]);            // seq 3 -- triggers gap detection
//! // heartbeats are sent automatically every second
//! h.shutdown();                               // end-of-session now replaces heartbeats
//! ```

use std::collections::BTreeMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread::spawn;
use std::time::Duration;

use bon::Builder;
use crossbeam::channel::{self, Receiver, RecvTimeoutError, Sender};
use tracing::{debug, error, info, warn};

const HEADER_LEN: usize = 20;
const HEARTBEAT: u16 = 0x0000;
const END_OF_SESSION: u16 = 0xFFFF;

type DB = Mutex<BTreeMap<u64, Vec<u8>>>;

/// Commands the running server processes from its input channel.
#[derive(Debug, Clone)]
pub enum ServerCommand {
    /// Build and broadcast a packet containing these messages.
    /// The packet is also stored in the retransmission log.
    Send(Vec<Vec<u8>>),
    /// Stage a packet in the log without broadcasting it. The next `Send`
    /// will leave a gap in the live stream that the client must re-request.
    SendDropped(Vec<Vec<u8>>),
    /// Send a heartbeat packet immediately (msg_count = 0). Heartbeats are
    /// also sent automatically once per second; this is for tests that need
    /// to force one at a specific moment. Does not advance the seq num.
    /// After [`StopSession`](Self::StopSession) this becomes an end-of-session.
    Heartbeat,
    /// Send a End-of-session packet (msg_count = 0xFFFF). Automatically sent in place of
    /// heartbeats after [`StopSession`](Self::StopSession).
    /// See [`Heartbeat`](Self::Heartbeat).
    EndOfSession,
    /// Server sends End-of-session packets in place of heartbeats.
    /// No new messages can be sent on this session.
    /// The re-request thread keeps running.
    StopSession,
    /// Changes session ident sent. Does not send End-of-session packet.
    /// Up to caller to inform clients of state change via [`StopSession`](Self::StopSession).
    ChangeSession(String),
}

#[derive(Builder)]
pub struct MoldUDP64Server {
    /// Destination for live packets. Use the multicast group + port for real
    /// runs; for tests over loopback unicast set this to the client's
    /// downstream bind address.
    multicast_addr: SocketAddrV4,
    /// Outbound interface for multicast. Ignored when the destination is
    /// unicast.
    #[builder(default = Ipv4Addr::UNSPECIFIED)]
    interface_addr: Ipv4Addr,
    /// Local bind address for the unicast re-request server. The client
    /// sends RetransmissionRequest datagrams here.
    rerequest_bind_addr: SocketAddr,
    /// 10-byte session identifier. Strings shorter than 10 bytes are
    /// right-padded with spaces; longer strings are truncated.
    session: String,
    #[builder(default = 1452)]
    /// Max size of frame transmitted.
    /// Default is 1452: 1500 - 20 (IP) - 8 (UDP) - 20 (Mold header)
    max_payload: usize,
    #[builder(default = Duration::from_secs(1))]
    heartbeat_interval: Duration,
    /// Number of commands in command queue before blocking.
    /// Set to 0 for unbuffered.
    #[builder(default = 1_000_000)]
    command_queue_size: usize,
    /// Initial sequence number of packets.
    #[builder(default = 1)]
    seq_num: u64,
}

pub struct ServerHandle {
    pub tx: Sender<ServerCommand>,
}

impl ServerHandle {
    pub fn send(&self, msgs: Vec<Vec<u8>>) {
        let _ = self.tx.send(ServerCommand::Send(msgs));
    }
    pub fn send_dropped(&self, msgs: Vec<Vec<u8>>) {
        let _ = self.tx.send(ServerCommand::SendDropped(msgs));
    }
    pub fn heartbeat(&self) {
        let _ = self.tx.send(ServerCommand::Heartbeat);
    }
    pub fn end_of_session(&self) {
        let _ = self.tx.send(ServerCommand::EndOfSession);
    }

    pub fn change_session(&self, session: String) {
        let _ = self.tx.send(ServerCommand::ChangeSession(session));
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(ServerCommand::StopSession);
    }
}

impl MoldUDP64Server {
    pub fn start(&self) -> io::Result<ServerHandle> {
        let downstream = UdpSocket::bind(SocketAddrV4::new(self.interface_addr, 0))?;
        let rereq = UdpSocket::bind(self.rerequest_bind_addr)?;
        self.start_with_sockets(downstream, rereq)
    }

    /// Test seam — accepts pre-bound sockets so tests can drive the server
    /// over loopback unicast without needing multicast support. The
    /// `multicast_addr` field is still used as the *destination* for sends.
    pub fn start_with_sockets(
        &self,
        downstream: UdpSocket,
        rereq: UdpSocket,
    ) -> io::Result<ServerHandle> {
        let session = pad_session(&self.session);
        let dest = SocketAddr::V4(self.multicast_addr);
        // Per-message log keyed by absolute seq num. We don't preserve packet
        // boundaries — re-requests synthesise a fresh packet from the range.
        let log: Arc<DB> = Arc::new(Mutex::new(BTreeMap::new()));
        let (cmd_tx, cmd_rx) = {
            if self.command_queue_size == 0 {
                channel::unbounded::<ServerCommand>()
            } else {
                channel::bounded::<ServerCommand>(1000_000)
            }
        };

        {
            let log = Arc::clone(&log);

            let max_payload = self.max_payload;
            let heartbeat_interval = self.heartbeat_interval;
            let seq_num = self.seq_num;
            spawn(move || {
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
            spawn(move || rerequest_loop(rereq, session, log, max_payload));
        }

        Ok(ServerHandle { tx: cmd_tx })
    }
}

fn pad_session(s: &str) -> [u8; 10] {
    let mut out = [b' '; 10];
    let bytes = s.as_bytes();
    let n = bytes.len().min(10);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}

fn build_packet(session: &[u8; 10], seq_num: u64, msgs: &[Vec<u8>]) -> Vec<u8> {
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
    let msg_count = if stopped { END_OF_SESSION } else { HEARTBEAT };
    let pkt = build_special(session, next_seq, msg_count);
    if let Err(e) = socket.send_to(&pkt, dest) {
        let kind = if stopped {
            "end-of-session"
        } else {
            "heartbeat"
        };
        error!("periodic {kind} send error: {e}");
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
                    store_messages(&log, next_seq, &msgs);
                    next_seq += count;
                    for msgs in chunk_messages(msgs, max_payload) {
                        let pkt = build_packet(&session, next_seq, &msgs);
                        if let Err(e) = socket.send_to(&pkt, dest) {
                            error!("downstream send error at seq {next_seq}: {e}");
                        }
                    }
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
                    store_messages(&log, next_seq, &msgs);
                    debug!("packet at seq {next_seq} ({count} msg(s)) staged but not sent");
                    next_seq += count;
                }
                ServerCommand::Heartbeat => {
                    // After StopSession, an explicit heartbeat becomes an EoS
                    // so it matches the behaviour of the periodic tick.
                    send_periodic(&socket, dest, &session, next_seq, stopped);
                }
                ServerCommand::EndOfSession => {
                    let pkt = build_special(&session, next_seq, END_OF_SESSION);
                    if let Err(e) = socket.send_to(&pkt, dest) {
                        error!("end-of-session send error: {e}");
                    }
                    info!("server: end-of-session at seq {next_seq}");
                }
                ServerCommand::ChangeSession(s) => {
                    if stopped {
                        warn!("ChangeSession ignored: session has been stopped");
                        continue;
                    }
                    session = pad_session(&s);
                }
                ServerCommand::StopSession => {
                    if stopped {
                        continue;
                    }
                    info!(
                        "server: stop-session at seq {next_seq}; \
                         end-of-session will be sent in place of heartbeats"
                    );
                    stopped = true;
                }
            },
            Err(RecvTimeoutError::Timeout) => {
                send_periodic(&socket, dest, &session, next_seq, stopped);
            }
            Err(RecvTimeoutError::Disconnected) => {
                debug!("server: command channel disconnected; sender loop exiting");
                break;
            }
        }
    }
}

fn rerequest_loop(socket: UdpSocket, session: [u8; 10], log: Arc<DB>, max_payload: usize) {
    let mut buf = [0u8; HEADER_LEN];
    loop {
        let (n, peer) = match socket.recv_from(&mut buf) {
            Ok(x) => x,
            Err(e) => {
                error!("rerequest recv error: {e}");
                break;
            }
        };
        if n < HEADER_LEN {
            warn!("short re-request from {peer}: {n} bytes");
            continue;
        }
        if buf[..10] != session[..] {
            warn!("session mismatch on re-request from {peer}");
            continue;
        }
        let start_seq = u64::from_be_bytes(buf[10..18].try_into().unwrap());
        let want = u16::from_be_bytes(buf[18..20].try_into().unwrap()) as u64;
        if want == 0 {
            continue;
        }

        let chunks: Vec<Vec<Vec<u8>>> = {
            // Collect contiguous messages from start_seq. If something is missing
            // we still send what we have — the client will re-ask for the rest.
            let log_g = log.lock().unwrap();
            let msgs = (0..want).map_while(|i| log_g.get(&(start_seq + i)).cloned());
            chunk_messages(msgs, max_payload)
        };

        if chunks.is_empty() {
            debug!("nothing in log for re-request from {peer} starting at {start_seq}");
            continue;
        }

        for msgs in chunks {
            let pkt = build_packet(&session, start_seq, &msgs);
            if let Err(e) = socket.send_to(&pkt, peer) {
                error!("retx send error to {peer}: {e}");
            } else {
                debug!(
                    "retransmitted {} msg(s) starting at seq {} to {}",
                    msgs.len(),
                    start_seq,
                    peer
                );
            }
        }
    }
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

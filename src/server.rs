//! Test server for the MoldUDP64 client.
//!
//! Broadcasts MoldUDP64 packets out one socket and answers re-request
//! traffic on another. Intended for tests, fuzzing, and local experimentation
//! — not production. The server keeps every individual message in an
//! in-memory log keyed by sequence number, so it can synthesise a response
//! to any range the client asks for without caring about original packet
//! boundaries.
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
//! h.heartbeat();
//! h.end_session();
//! ```

use std::collections::BTreeMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread::spawn;

use bon::Builder;
use crossbeam::channel::{self, Receiver, Sender};
use tracing::{debug, error, info, warn};

const HEADER_LEN: usize = 20;
const HEARTBEAT: u16 = 0x0000;
const END_OF_SESSION: u16 = 0xFFFF;

/// Commands the running server processes from its input channel.
#[derive(Debug, Clone)]
pub enum ServerCommand {
    /// Build and broadcast a packet containing these messages.
    /// The packet is also stored in the retransmission log.
    Send(Vec<Vec<u8>>),
    /// Stage a packet in the log without broadcasting it. The next `Send`
    /// will leave a gap in the live stream that the client must re-request.
    SendDropped(Vec<Vec<u8>>),
    /// Heartbeat packet (msg_count = 0). Does not advance the seq num.
    Heartbeat,
    /// End-of-session packet (msg_count = 0xFFFF). The sender thread exits
    /// once this is sent. The re-request thread keeps running.
    EndSession,
    /// Changes session ident sent.
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
    pub fn end_session(&self) {
        let _ = self.tx.send(ServerCommand::EndSession);
    }

    pub fn change_session(&self, session: String) {
        let _ = self.tx.send(ServerCommand::ChangeSession(session));
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
        let log: Arc<Mutex<BTreeMap<u64, Vec<u8>>>> = Arc::new(Mutex::new(BTreeMap::new()));
        let (cmd_tx, cmd_rx) = channel::unbounded::<ServerCommand>();

        {
            let log = Arc::clone(&log);
            spawn(move || sender_loop(downstream, dest, session, log, cmd_rx));
        }
        {
            let log = Arc::clone(&log);
            spawn(move || rerequest_loop(rereq, session, log));
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

fn store_messages(log: &Mutex<BTreeMap<u64, Vec<u8>>>, start_seq: u64, msgs: &[Vec<u8>]) {
    let mut g = log.lock().unwrap();
    for (i, m) in msgs.iter().enumerate() {
        g.insert(start_seq + i as u64, m.clone());
    }
}

fn sender_loop(
    socket: UdpSocket,
    dest: SocketAddr,
    mut session: [u8; 10],
    log: Arc<Mutex<BTreeMap<u64, Vec<u8>>>>,
    cmd_rx: Receiver<ServerCommand>,
) {
    let mut next_seq: u64 = 1; // MoldUDP64 seq nums are 1-based
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            ServerCommand::Send(msgs) => {
                if msgs.is_empty() {
                    warn!("empty Send ignored; use Heartbeat instead");
                    continue;
                }
                let count = msgs.len() as u64;
                store_messages(&log, next_seq, &msgs);
                let pkt = build_packet(&session, next_seq, &msgs);
                if let Err(e) = socket.send_to(&pkt, dest) {
                    error!("downstream send error at seq {next_seq}: {e}");
                }
                next_seq += count;
            }
            ServerCommand::SendDropped(msgs) => {
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
                let pkt = build_special(&session, next_seq, HEARTBEAT);
                if let Err(e) = socket.send_to(&pkt, dest) {
                    error!("heartbeat send error: {e}");
                }
                // heartbeats do NOT advance next_seq (per spec)
            }
            ServerCommand::EndSession => {
                let pkt = build_special(&session, next_seq, END_OF_SESSION);
                if let Err(e) = socket.send_to(&pkt, dest) {
                    error!("end-of-session send error: {e}");
                }
                info!("server: end-of-session at seq {next_seq}");
                break;
            }
            ServerCommand::ChangeSession(s) => {
                session = pad_session(&s);
            }
        }
    }
}

fn rerequest_loop(socket: UdpSocket, session: [u8; 10], log: Arc<Mutex<BTreeMap<u64, Vec<u8>>>>) {
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

        // Collect contiguous messages from start_seq. If something is missing
        // we still send what we have — the client will re-ask for the rest.
        let msgs: Vec<Vec<u8>> = {
            let log_g = log.lock().unwrap();
            (0..want)
                .map_while(|i| log_g.get(&(start_seq + i)).cloned())
                .collect()
        };

        if msgs.is_empty() {
            debug!("nothing in log for re-request from {peer} starting at {start_seq}");
            continue;
        }

        // NOTE: a real server would chunk the response by MTU. Tests assume
        // small messages so we emit one packet covering everything we have.
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

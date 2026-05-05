use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;

/// Replay an ITCH5 file over MoldUDP64: the server thread streams packets to a
/// multicast group while the client thread receives them and builds order books.
#[derive(clap::Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Config {
    /// Path to itch5 file to replay.
    #[arg(short = 'f', long = "file")]
    pub file_path: PathBuf,

    /// Path to file to write output.
    #[arg(short = 'o', long = "out")]
    pub output_file_path: Option<PathBuf>,

    /// Symbols to watch
    #[arg(short = 'w', long = "watch")]
    pub symbol_strs: Option<Vec<String>>,

    /// Session identifier. Strings shorter than 10 bytes are right-padded with
    /// spaces; longer strings are truncated.
    #[arg(short = 's', long, default_value = DEFAULT_SESSION)]
    pub session: String,

    /// Multicast group + port shared by the server (sender) and client (receiver).
    #[arg(short = 'm', long, default_value_t = DEFAULT_MULTICAST_ADDR.parse().unwrap())]
    pub multicast_addr: SocketAddrV4,

    /// Local bind address for the unicast re-request server. The client also
    /// sends RetransmissionRequest datagrams to this address.
    #[arg(short = 'r', long, default_value_t = DEFAULT_REREQUEST_ADDR.parse().unwrap())]
    pub rerequest_addr: SocketAddr,

    /// Local interface used by the client to join the multicast group.
    #[arg(short = 'i', long, default_value_t = Ipv4Addr::UNSPECIFIED)]
    pub interface_addr: Ipv4Addr,

    /// Max number of messages to send per batch.
    /// Note: Depending on total length of batch, the batch may be sent in
    /// separate packets due to MTU.
    #[arg(long, default_value_t = DEFAULT_MAX_MSGS)]
    pub max_msgs: usize,
}

const DEFAULT_MULTICAST_ADDR: &str = "239.1.2.3:5000";
const DEFAULT_REREQUEST_ADDR: &str = "127.0.0.1:6000";
const DEFAULT_SESSION: &str = "TESTSESSN";
const DEFAULT_MAX_MSGS: usize = 100;

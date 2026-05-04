use std::error::Error;

/// Errors that can be returned when parsing MoldUDP64 packets.
#[derive(Debug)]
pub enum MoldUdpError {
    /// The packet header or message block contains data that violates the
    /// MoldUDP64 specification (e.g. a length field that exceeds the
    /// remaining buffer).
    MalformedPacket { msg: &'static str },

    /// The message block ends before the length prefix indicated.
    /// This typically means the UDP datagram was truncated at the network
    /// layer or was corrupted in transit.
    TruncatedMessage,
}

impl std::fmt::Display for MoldUdpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedPacket { msg } => writeln!(f, "Malformed MoldUdp64 packet: {msg}"),
            Self::TruncatedMessage => writeln!(f, "Truncated message"),
        }
    }
}

impl Error for MoldUdpError {}

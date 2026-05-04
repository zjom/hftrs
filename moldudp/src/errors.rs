use thiserror::Error;

/// Errors that can be returned when parsing MoldUDP64 packets.
#[derive(Debug, Error)]
pub enum MoldUdpError {
    /// The packet header or message block contains data that violates the
    /// MoldUDP64 specification (e.g. a length field that exceeds the
    /// remaining buffer).
    #[error("Malformed MoldUdp64 packet: {msg}")]
    MalformedPacket { msg: &'static str },

    /// The message block ends before the length prefix indicated.
    /// This typically means the UDP datagram was truncated at the network
    /// layer or was corrupted in transit.
    #[error("Truncated message")]
    TruncatedMessage,
}

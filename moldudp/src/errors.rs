use thiserror::Error;

#[derive(Debug, Error)]
pub enum MoldUdpError {
    #[error("Malformed MoldUdp64 packet: {msg}")]
    MalformedPacket { msg: &'static str },
    #[error("Truncated message")]
    TruncatedMessage,
}

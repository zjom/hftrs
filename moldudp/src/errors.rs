use std::{error::Error, fmt::Display};

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

/// Errors that can be returned when intialising a socket.
#[derive(Debug)]
pub(crate) enum SocketInitError {
    /// `setsockopt` syscall to set `SO_RCVBUF` failed.
    SetRcvBufSizeError {
        error: std::io::Error,
        socket_label: &'static str,
        desired_size: usize,
    },

    /// The kernel clamped `SO_RCVBUF` to `net.core.rmem_max` (often 208 KiB by default).
    KernelClamp {
        socket_label: &'static str,
        desired_size: usize,
    },

    /// `getsockopt` syscall to get `SO_RCVBUF` failed.
    /// Unable to verify actual `SO_RCVBUF` size.
    /// Packets may be truncated silently.
    GetRcvBufSizeError {
        error: std::io::Error,
        socket_label: &'static str,
    },
}

impl Display for SocketInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetRcvBufSizeError {
                error,
                socket_label,
                desired_size,
            } => writeln!(
                f,
                "failed to set `SO_RCVBUF` to {desired_size} for {socket_label} due to {error}"
            ),
            Self::KernelClamp {
                socket_label,
                desired_size,
            } => writeln!(
                f,
                "failed to set `SO_RCVBUF` for {socket_label} due to kernel clamp; on Linux raise net.core.rmem_max with `sudo sysctl -w net.core.rmem_max={desired_size}`"
            ),
            Self::GetRcvBufSizeError {
                error,
                socket_label,
            } => writeln!(
                f,
                "failed to verify `SO_RCVBUF` for {socket_label} due to {error}. packets may be silently dropped"
            ),
        }
    }
}

impl Error for SocketInitError {}

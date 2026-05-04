use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned,
    big_endian::{U16, U64},
};

/// `msg_count` value that identifies a heartbeat packet.
///
/// Heartbeats are sent periodically (typically once per second) by the server
/// so receivers can sense packet loss even during times of low traffic. A
/// heartbeat packet carries the sequence number of the *next* expected message
/// but contains no message data.
pub const HEARTBEAT_IDENT: u16 = 0;

/// `msg_count` value that identifies an end-of-session packet.
///
/// When the current session is complete, the server sends downstream packets
/// with this value in place of heartbeats for a short window. Like heartbeats,
/// they carry the next expected sequence number but no message data. While
/// these packets are flowing, clients may still re-request missing messages;
/// it is the last opportunity to close any gaps.
pub const END_OF_SESSION_IDENT: u16 = u16::MAX;

/// The type of a downstream packet based on its `msg_count` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketKind {
    /// A normal data packet carrying one or more messages
    /// (`msg_count > 0` and `msg_count != 0xFFFF`).
    Standard,
    /// A liveness probe with no messages (`msg_count == 0`).
    /// Contains the next expected sequence number.
    Heartbeat,
    /// The session has ended (`msg_count == 0xFFFF`).
    /// Contains the next expected sequence number. Re-requests are still
    /// accepted while these packets are being sent.
    EndOfSession,
}

/// Whether the session associated with a packet is still accepting messages.
///
/// A session is *active* from its first message until [`ServerHandle::shutdown`]
/// is called. Once a session ends, no new messages can be sent on it.
///
/// [`ServerHandle::shutdown`]: crate::ServerHandle::shutdown
pub enum SessionStatus {
    /// The session has started and has not yet been terminated.
    Active,
    /// The session is over; the packet's `msg_count` is `0xFFFF`.
    Inactive,
}

/// Fixed 20-byte downstream packet header.
///
/// All multi-byte fields are big-endian (network byte order).
///
/// Layout (per the MoldUDP64 spec):
///
/// ```text
/// Offset  Size  Field
/// ───────────────────────────────────────────────────────
///  0       10   Session identifier (ASCII, space-padded)
/// 10        8   Sequence number of first message (u64)
/// 18        2   Message count (u16)
/// ───────────────────────────────────────────────────────
/// ```
///
/// This type is also used as a [`RetransmissionPacket`] when requesting a
/// gap fill: set `session` and `seq_num` to the start of the gap and
/// `msg_count` to the number of messages wanted (capped at `u16::MAX`).
///
/// [`RetransmissionPacket`]: crate::RetransmissionPacket
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
pub struct PacketHeader {
    /// 10-byte ASCII session identifier, right-padded with spaces.
    pub session: [u8; 10],
    /// Sequence number of the first message carried in this packet.
    pub seq_num: U64,
    /// Number of message blocks that follow the header.
    /// `0` = heartbeat; `0xFFFF` = end-of-session.
    pub msg_count: U16,
}

/// A full downstream packet: a 20-byte header followed by 0–N message blocks.
///
/// `Packet` is a [dynamically-sized type](https://doc.rust-lang.org/reference/dynamically-sized-types.html)
/// built with [`zerocopy`]. Construct a zero-copy reference from a byte slice
/// using [`Packet::parse`] or the lower-level [`zerocopy::FromBytes::ref_from_bytes`].
///
/// # Example
///
/// ```no_run
/// use moldudp::{FromBytes, Packet, PacketKind};
///
/// fn handle(raw: &[u8]) {
///     let packet = Packet::ref_from_bytes(raw).expect("invalid packet");
///
///     match packet.packet_kind() {
///         PacketKind::Heartbeat | PacketKind::EndOfSession => return,
///         PacketKind::Standard => {}
///     }
///
///     for msg in packet.iter() {
///         // msg.data() is a zero-copy view into `raw` — no allocation.
///         process(msg.data());
///     }
/// }
///
/// fn process(_data: &[u8]) {}
/// ```
#[derive(Debug, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
pub struct Packet {
    pub header: PacketHeader,
    /// Raw bytes of the message blocks. Iterate via [`Packet::iter`] rather
    /// than accessing this field directly.
    pub messages: [u8],
}

impl Packet {
    /// The minimum valid wire size of a MoldUDP64 packet (header only, no messages).
    pub(crate) const MIN_PACKET_LEN: usize = 20;

    /// Wraps `bytes` as a `&Packet` without copying.
    ///
    /// Returns `None` if `bytes.len() < 20` or if the slice cannot be
    /// interpreted as a valid packet layout.
    #[inline]
    pub fn parse(bytes: &[u8]) -> Option<&Self> {
        Self::ref_from_bytes(bytes).ok()
    }

    /// Session identifier decoded as a UTF-8 string.
    ///
    /// Returns an error if the 10 session bytes are not valid UTF-8.
    /// Use [`session_ident_raw`](Self::session_ident_raw) to access the bytes
    /// unconditionally.
    #[inline]
    pub fn session_ident(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.header.session)
    }

    /// Raw 10-byte session identifier, without UTF-8 validation.
    #[inline]
    pub fn session_ident_raw(&self) -> &[u8; 10] {
        &self.header.session
    }

    /// Sequence number of the *first* message in this packet.
    ///
    /// Sequence numbers start at 1 and increase monotonically within a
    /// session. For heartbeat and end-of-session packets this is the
    /// sequence number of the *next* expected message.
    #[inline]
    pub fn seq_num(&self) -> u64 {
        self.header.seq_num.get()
    }

    /// Number of message blocks carried by this packet.
    ///
    /// - `0` ([`HEARTBEAT_IDENT`]) — heartbeat; no messages.
    /// - `0xFFFF` ([`END_OF_SESSION_IDENT`]) — end-of-session; no messages.
    /// - Any other value — standard data packet with that many messages.
    #[inline]
    pub fn msg_count(&self) -> u16 {
        self.header.msg_count.get()
    }

    /// Whether the session that produced this packet is still active.
    #[inline]
    pub fn session_status(&self) -> SessionStatus {
        match self.msg_count() {
            END_OF_SESSION_IDENT => SessionStatus::Inactive,
            _ => SessionStatus::Active,
        }
    }

    /// Classify this packet as [`Standard`], [`Heartbeat`], or
    /// [`EndOfSession`] based on `msg_count`.
    ///
    /// [`Standard`]: PacketKind::Standard
    /// [`Heartbeat`]: PacketKind::Heartbeat
    /// [`EndOfSession`]: PacketKind::EndOfSession
    #[inline]
    pub fn packet_kind(&self) -> PacketKind {
        match self.msg_count() {
            HEARTBEAT_IDENT => PacketKind::Heartbeat,
            END_OF_SESSION_IDENT => PacketKind::EndOfSession,
            _ => PacketKind::Standard,
        }
    }

    /// Returns a zero-allocation iterator over the message blocks in this
    /// packet.
    ///
    /// Yields nothing for heartbeat or end-of-session packets. Also
    /// implements [`IntoIterator`], so `for msg in &packet { … }` works too.
    #[inline]
    pub fn iter(&self) -> Messages<'_> {
        Messages {
            bytes: &self.messages,
            remaining: match self.msg_count() {
                END_OF_SESSION_IDENT | HEARTBEAT_IDENT => 0,
                n => n,
            },
        }
    }
}

impl<'a> IntoIterator for &'a Packet {
    type Item = &'a Message;
    type IntoIter = Messages<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A single length-prefixed message block inside a downstream packet.
///
/// The wire layout is a 2-byte big-endian length field followed by that many
/// bytes of payload. The contents of the payload are application-defined;
/// MoldUDP64 treats them as opaque bytes.
///
/// Construct via [`Packet::iter`] — never directly.
#[derive(Debug, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
pub struct Message {
    length: U16,
    data: [u8],
}

impl Message {
    /// Number of payload bytes in this message (not including the 2-byte
    /// length prefix itself).
    #[inline]
    pub fn length(&self) -> u16 {
        self.length.get()
    }

    /// Zero-copy view of the message payload.
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

/// Zero-allocation iterator over the [`Message`] blocks in a [`Packet`].
///
/// Obtained via [`Packet::iter`] or by iterating `&packet` directly.
/// Implements [`ExactSizeIterator`], so `.len()` returns the remaining
/// message count in O(1).
pub struct Messages<'a> {
    bytes: &'a [u8],
    remaining: u16,
}

impl<'a> Iterator for Messages<'a> {
    type Item = &'a Message;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        // Peek at the 2-byte length field to know how many trailing bytes
        // belong to this message block.
        let (len_field, _) = U16::ref_from_prefix(self.bytes).ok()?;
        let length = len_field.get() as usize;

        // Re-parse the prefix as a full Message DST with `length` trailing bytes.
        let (msg, rest) = Message::ref_from_prefix_with_elems(self.bytes, length).ok()?;
        self.bytes = rest;
        self.remaining -= 1;
        Some(msg)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.remaining as usize;
        (n, Some(n))
    }
}

impl<'a> ExactSizeIterator for Messages<'a> {}

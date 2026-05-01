use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned,
    big_endian::{U16, U64},
};

/// Heartbeats are sent periodically by the server so receivers can sense packet loss even during times of low traffic.
/// Typically, these packets are transmitted once per second and contain the next expected Sequence Number.
/// A Heartbeat packet is a MoldUDP64 packet with a Message Count of zero.
pub const HEARTBEAT_IDENT: u16 = 0;

/// When the current session is complete, Downstream Packets are sent with a Message Count of `0xFFFF` for a short while in place of Heartbeats.
/// These Downstream Packets contain the next expected Sequence Number, just like Heartbeats.
/// While the End of Session messages persist, re-requests may be made on the current session.
/// This is the last chance to ensure that all messages have been received.
pub const END_OF_SESSION_IDENT: u16 = u16::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketKind {
    Standard,
    Heartbeat,
    EndOfSession,
}

/// A Session is a sequence of one or more messages.
/// While a single session can last indefinitely, typically the application will
/// define a session to logically group messages together based on time delimitation.
/// Once a session is terminated, no more messages can be sent on that session.
/// Depending on the design of the MoldUDP64 system and the application,
/// receivers may still be able to re-request messages from a terminated session.
/// A session is considered active if it has started but not yet been terminated.
/// Indicates the session to which this packet belongs.
pub enum SessionStatus {
    Active,
    Inactive,
}

/// Fixed 20-byte downstream packet header.
///
/// Layout (per the MoldUDP64 spec):
/// - `[0..10]`  Session
/// - `[10..18]` Sequence Number (big-endian u64)
/// - `[18..20]` Message Count   (big-endian u16)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
pub struct PacketHeader {
    pub session: [u8; 10],
    pub seq_num: U64,
    pub msg_count: U16,
}

/// Full downstream packet: a header followed by 0..N message blocks.
///
/// Construct one with [`Packet::parse`] or directly via [`Packet::ref_from_bytes`].
/// A MoldUDP64 transmitter sends “downstream” packets that are received by MoldUDP64 listeners. A MoldUDP64 packet may contain a payload of 0 or more data stream messages.
///Each MoldUDP64 packet consists of a Downstream Packet Header and of a series of Message Blocks. The Message Blocks carry the actual data of the stream. See [`Message`].
#[derive(Debug, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
pub struct Packet {
    pub header: PacketHeader,
    /// Raw bytes of the messages block. Iterate via [`Packet::iter`].
    pub messages: [u8],
}

impl Packet {
    /// Wraps the bytes of a downstream packet without copying.
    /// Returns `None` if `bytes.len() < 20`.
    #[inline]
    pub fn parse(bytes: &[u8]) -> Option<&Self> {
        Self::ref_from_bytes(bytes).ok()
    }

    /// Session identifier as `&str`. Errors if the 10 bytes aren't valid UTF-8.
    #[inline]
    pub fn session_ident(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.header.session)
    }

    /// Raw 10 bytes of the session identifier.
    ///
    /// See [`Self::session_ident`] for more information.
    #[inline]
    pub fn session_ident_raw(&self) -> &[u8; 10] {
        &self.header.session
    }

    /// Sequence number of the first message in the packet.
    #[inline]
    pub fn seq_num(&self) -> u64 {
        self.header.seq_num.get()
    }

    /// Count of messages in the packet.
    /// - `0xFFFF` indicates end of session.
    /// - `0x0` indicates heartbeat.
    #[inline]
    pub fn msg_count(&self) -> u16 {
        self.header.msg_count.get()
    }

    #[inline]
    pub fn session_status(&self) -> SessionStatus {
        match self.msg_count() {
            END_OF_SESSION_IDENT => SessionStatus::Inactive,
            _ => SessionStatus::Active,
        }
    }

    #[inline]
    pub fn packet_kind(&self) -> PacketKind {
        match self.msg_count() {
            HEARTBEAT_IDENT => PacketKind::Heartbeat,
            END_OF_SESSION_IDENT => PacketKind::EndOfSession,
            _ => PacketKind::Standard,
        }
    }

    /// Zero-allocation iterator over the message blocks.
    ///
    /// Yields nothing for heartbeat or end-of-session packets.
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

    pub(crate) const MIN_PACKET_LEN: usize = 20;
}

impl<'a> IntoIterator for &'a Packet {
    type Item = &'a Message;
    type IntoIter = Messages<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A length-prefixed message block: 2-byte big-endian length + payload.
///
/// A message is an atomic piece of information carried by the MoldUDP64 protocol.
/// MoldUDP64 can theoretically handle individual messages from zero bytes up
/// to 64KB in length although individual messages should be kept small enough so
/// that the UDP underlying network protocol can efficiently carry the resulting
/// MoldUDP64 packets.
/// The contents of a MoldUDP64 message are defined by the higher level application.
#[derive(Debug, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C)]
pub struct Message {
    length: U16,
    data: [u8],
}

impl Message {
    #[inline]
    pub fn length(&self) -> u16 {
        self.length.get()
    }

    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

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

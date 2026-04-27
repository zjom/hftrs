/// A MoldUDP64 transmitter sends “downstream” packets that are received by MoldUDP64 listeners.
/// A MoldUDP64 packet may contain a payload of 0 or more data stream messages.
/// Each MoldUDP64 packet consists of a Downstream Packet Header and of a series of Message Blocks.
/// The Message Blocks carry the actual data of the stream.
pub struct Packet<'a>(pub &'a [u8]);

pub enum PacketKind {
    Standard,
    Heartbeat,
    EndOfSession,
}

/// When the current session is complete, Downstream Packets are sent with a Message Count of 0xFFFF(hex,
/// or 65535 in decimal) for a short while in place of Heartbeats. These Downstream Packets contain the next
/// expected Sequence Number, just like Heartbeats. While the End of Session messages persist, re-requests may
/// be made on the current session. This is the last chance to ensure that all messages have been received.
pub const END_OF_SESSION_IDENT: u16 = u16::MAX;

/// Heartbeats are sent periodically by the server so receivers can sense packet loss even during times of low
/// traffic. Typically, these packets are transmitted once per second and contain the next expected Sequence
/// Number. A Heartbeat packet is a MoldUDP64 packet with a Message Count of zero.
pub const HEARTBEAT_IDENT: u16 = 0;

/// A message is an atomic piece of information carried by the MoldUDP64 protocol.
/// MoldUDP64 can theoretically handle individual messages from zero bytes up
/// to 64KB in length although individual messages should be kept small enough so
/// that the UDP underlying network protocol can efficiently carry the resulting
/// MoldUDP64 packets.
/// The contents of a MoldUDP64 message are defined by the higher level application.
/// A message block: 2-byte big-endian length followed by `length` bytes of payload.
pub struct Message<'a>(pub &'a [u8]);

pub enum SessionStatus {
    Active,
    Inactive,
}
impl<'a> Packet<'a> {
    /// Wraps the bytes of a downstream packet, providing utilities to inspect the data.
    /// Does not allocate anything.
    /// Bytes must have length of at least 20.
    ///
    /// A packet is composed of header block + optional messages block.
    /// Header:
    /// - Session: `[0..10]`
    /// - Sequence Number: `[10..18]`
    /// - Message Count: `[18..20]`
    /// Messages (Optional):
    /// - Message: 2-byte big-endian length followed by `length` bytes of payload.
    pub const fn new(bytes: &'a [u8]) -> Packet<'a> {
        debug_assert!(bytes.len() >= Self::MIN_PACKET_LEN);
        Packet(bytes)
    }

    pub(crate) const MIN_PACKET_LEN: usize = 20;

    /// A Session is a sequence of one or more messages.
    /// While a single session can last indefinitely, typically the application will
    /// define a session to logically group messages together based on time delimitation.
    /// Once a session is terminated, no more messages can be sent on that session.
    /// Depending on the design of the MoldUDP64 system and the application,
    /// receivers may still be able to re-request messages from a terminated session.
    /// A session is considered active if it has started but not yet been terminated.
    /// Indicates the session to which this packet belongs.
    #[inline]
    pub const fn session_ident(&self) -> &'a str {
        match self.0.split_at(10).0 {
            bytes => match std::str::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => panic!("invalid UTF-8 in session"),
            },
        }
    }

    #[inline]
    /// Raw bytes of session identifier. See [`Self::session_ident`] for more information.
    pub fn session_ident_raw(&self) -> &'a [u8; 10] {
        self.0.split_at(10).0.try_into().unwrap()
    }

    /// When the current session is complete, Downstream Packets are sent with a Message Count of 0xFFFF
    /// (hex,or 65535 in decimal) for a short while in place of Heartbeats. These Downstream Packets contain the next
    /// expected Sequence Number, just like Heartbeats. While the End of Session messages persist, re-requests may
    /// be made on the current session. This is the last chance to ensure that all messages have been received.
    #[inline]
    pub const fn session_status(&self) -> SessionStatus {
        match self.msg_count() {
            END_OF_SESSION_IDENT => SessionStatus::Inactive,
            _ => SessionStatus::Active,
        }
    }
    /// The sequence number of the first message in the packet.
    #[inline]
    pub const fn seq_num(&self) -> u64 {
        u64::from_be_bytes([
            self.0[10], self.0[11], self.0[12], self.0[13], self.0[14], self.0[15], self.0[16],
            self.0[17],
        ])
    }
    /// The count of messages contained in this packet.
    /// A message count of 0xFFFF indicates end of session.
    #[inline]
    pub const fn msg_count(&self) -> u16 {
        u16::from_be_bytes([self.0[18], self.0[19]])
    }

    /// The bytes of the messages block contained in this packet.
    #[inline]
    pub const fn messages(&self) -> &'a [u8] {
        self.0.split_at(20).1
    }

    #[inline]
    pub const fn packet_kind(&self) -> PacketKind {
        use PacketKind as PK;
        match self.msg_count() {
            HEARTBEAT_IDENT => PK::Heartbeat,
            END_OF_SESSION_IDENT => PK::EndOfSession,
            _ => PK::Standard,
        }
    }

    /// Returns a 0 allocation iterator that loops through messages in this packet.
    #[inline]
    pub const fn iter(&self) -> Messages<'a> {
        Messages {
            bytes: self.messages(),
            // 0xFFFF means end-of-session, no real messages follow
            remaining: match self.msg_count() {
                END_OF_SESSION_IDENT | HEARTBEAT_IDENT => 0,
                n => n,
            },
        }
    }
}

impl<'a> IntoIterator for &Packet<'a> {
    type Item = Message<'a>;
    type IntoIter = Messages<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> Message<'a> {
    #[inline]
    pub const fn length(&self) -> u16 {
        u16::from_be_bytes([self.0[0], self.0[1]])
    }

    #[inline]
    pub const fn data(&self) -> &'a [u8] {
        self.0.split_at(2).1
    }
}

pub struct Messages<'a> {
    bytes: &'a [u8],
    remaining: u16,
}

impl<'a> Iterator for Messages<'a> {
    type Item = Message<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 || self.bytes.len() < 2 {
            return None;
        }
        let len = u16::from_be_bytes([self.bytes[0], self.bytes[1]]) as usize;
        let total = 2 + len;
        if self.bytes.len() < total {
            return None; // truncated packet
        }
        let (block, rest) = self.bytes.split_at(total);
        self.bytes = rest;
        self.remaining -= 1;
        Some(Message(block))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.remaining as usize;
        (n, Some(n))
    }
}

impl<'a> ExactSizeIterator for Messages<'a> {}

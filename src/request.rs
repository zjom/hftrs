use std::array;

/// The Request Packet is sent to request the retransmission of a particular message or group of messages. The
/// request packet is sent to a Re-request server. A receiver may need to send this request when it detects a
/// sequence number gap in received messages. The response to a valid Request Packet is a standard Downstream
/// Packet unicast back to the source of the retransmission request. This allows downstream MoldUDP64 users to
/// read the retransmitted Downstream Packet in their multicast processing socket if the request was made from
/// that socket (in other words, the client need only have one socket open to listen to the multicast and to process
/// retransmissions, even though the retransmissions are not multicast).

pub struct Request([u8; 20]);
impl Request {
    pub fn new(session_ident: &str, seq_num: u64, msg_count: u16) -> Request {
        let mut buf: [u8; 20] = array::repeat(0);

        let bytes = session_ident.as_bytes();
        debug_assert!(bytes.len() <= Self::SESSION_LENGTH);
        let end = Self::SESSION_OFFSET + bytes.len();
        buf[Self::SESSION_OFFSET..end].copy_from_slice(bytes);

        let end = Self::SEQ_OFFSET + Self::SEQ_LENGTH;
        buf[Self::SEQ_OFFSET..end].copy_from_slice(&seq_num.to_be_bytes());

        let end = Self::MSG_COUNT_OFFSET + Self::MSG_COUNT_LENGTH;
        buf[Self::MSG_COUNT_OFFSET..end].copy_from_slice(&msg_count.to_be_bytes());

        Request(buf)
    }
    pub const fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    const SESSION_OFFSET: usize = 0;
    const SESSION_LENGTH: usize = 10;

    const SEQ_OFFSET: usize = 10;
    const SEQ_LENGTH: usize = 8;

    const MSG_COUNT_OFFSET: usize = 18;
    const MSG_COUNT_LENGTH: usize = 2;
}

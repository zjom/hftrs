use crate::Message;
use zerocopy::{FromBytes, Immutable, KnownLayout};

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    EmptyBuffer,
    UnknownMessageType(u8),
    MalformedData,
}

pub fn parse<'a>(buf: &'a [u8]) -> Result<(Message<'a>, &'a [u8]), ParseError> {
    let (msg_len_bytes, buf) = buf.split_at_checked(2).ok_or(ParseError::EmptyBuffer)?;
    let msg_len = u16::from_be_bytes(msg_len_bytes.try_into().unwrap()) as usize;
    let msg_type = buf.first().ok_or(ParseError::EmptyBuffer)?;

    match msg_type {
        b'S' => try_parse(buf, msg_len).map(|(m, rem)| (Message::SystemEventMessage(m), rem)),
        b'R' => try_parse(buf, msg_len).map(|(m, rem)| (Message::StockDirectory(m), rem)),
        b'H' => try_parse(buf, msg_len).map(|(m, rem)| (Message::StockTradingAction(m), rem)),
        b'Y' => try_parse(buf, msg_len).map(|(m, rem)| (Message::RegSHORestriction(m), rem)),
        b'L' => {
            try_parse(buf, msg_len).map(|(m, rem)| (Message::MarketParticipantPosition(m), rem))
        }
        b'V' => try_parse(buf, msg_len).map(|(m, rem)| (Message::MWCBDeclineLevelMessage(m), rem)),
        b'W' => try_parse(buf, msg_len).map(|(m, rem)| (Message::MWCBStatusMessage(m), rem)),
        b'K' => try_parse(buf, msg_len).map(|(m, rem)| (Message::QuotingPeriodUpdate(m), rem)),
        b'J' => try_parse(buf, msg_len).map(|(m, rem)| (Message::LULDAuctionCollar(m), rem)),
        b'h' => try_parse(buf, msg_len).map(|(m, rem)| (Message::OperationalHalt(m), rem)),
        b'A' => {
            try_parse(buf, msg_len).map(|(m, rem)| (Message::AddOrderNoMPIDAttribution(m), rem))
        }
        b'F' => {
            try_parse(buf, msg_len).map(|(m, rem)| (Message::AddOrderWithMPIDAttribution(m), rem))
        }
        b'E' => try_parse(buf, msg_len).map(|(m, rem)| (Message::OrderExecutedMessage(m), rem)),
        b'C' => {
            try_parse(buf, msg_len).map(|(m, rem)| (Message::OrderExecutedWithPriceMessage(m), rem))
        }
        b'X' => try_parse(buf, msg_len).map(|(m, rem)| (Message::OrderCancelMessage(m), rem)),
        b'D' => try_parse(buf, msg_len).map(|(m, rem)| (Message::OrderDeleteMessage(m), rem)),
        b'U' => try_parse(buf, msg_len).map(|(m, rem)| (Message::OrderReplaceMessage(m), rem)),
        b'P' => try_parse(buf, msg_len).map(|(m, rem)| (Message::TradeMessage(m), rem)),
        b'Q' => try_parse(buf, msg_len).map(|(m, rem)| (Message::CrossTradeMessage(m), rem)),
        b'B' => try_parse(buf, msg_len).map(|(m, rem)| (Message::BrokenTradeMessage(m), rem)),
        b'I' => try_parse(buf, msg_len)
            .map(|(m, rem)| (Message::NetOrderImbalanceIndicatorMessage(m), rem)),
        b'N' => try_parse(buf, msg_len)
            .map(|(m, rem)| (Message::RetailPriceImprovementIndicator(m), rem)),
        b'O' => try_parse(buf, msg_len).map(|(m, rem)| {
            (
                Message::DirectListingwithCapitalRaisePriceDiscoveryMessage(m),
                rem,
            )
        }),

        unknown => Err(ParseError::UnknownMessageType(*unknown)),
    }
}

fn try_parse<'a, T: FromBytes + KnownLayout + Immutable>(
    buf: &'a [u8],
    msg_len: usize,
) -> Result<(&'a T, &'a [u8]), ParseError> {
    let (body, rest) = buf
        .split_at_checked(msg_len)
        .ok_or(ParseError::MalformedData)?;
    let msg = T::ref_from_bytes(body).map_err(|_| ParseError::MalformedData)?;
    Ok((msg, rest))
}

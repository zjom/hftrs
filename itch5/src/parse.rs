use crate::messages::*;
use std::ops::ControlFlow;
use thiserror::Error;
use zerocopy::{FromBytes, Immutable, KnownLayout};

#[derive(Debug, PartialEq, Eq, Error)]
pub enum ParseError {
    #[error("attempted to parse empty input")]
    EmptyBuffer,
    #[error("unknown message type {0}")]
    UnknownMessageType(u8),
    #[error("malformed input data")]
    MalformedData,
}

/// Visitor called by [`Parser`] for each decoded ITCH 5.0 message.
///
/// Every method has a default no-op implementation that returns
/// `ControlFlow::Continue(())`, so you only need to override the message
/// types you care about.  Return `ControlFlow::Break(())` from any method
/// to stop parsing immediately.
pub trait MessageHandler {
    fn on_system_event_message(&mut self, _msg: &SystemEventMessage) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_stock_directory(&mut self, _msg: &StockDirectory) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_stock_trading_action(&mut self, _msg: &StockTradingAction) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_reg_sho_restriction(&mut self, _msg: &RegSHORestriction) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_market_participant_position(
        &mut self,
        _msg: &MarketParticipantPosition,
    ) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_mwcb_decline_level_message(&mut self, _msg: &MWCBDeclineLevelMessage) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_mwcb_status_message(&mut self, _msg: &MWCBStatusMessage) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_quoting_period_update(&mut self, _msg: &QuotingPeriodUpdate) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_luld_auction_collar(&mut self, _msg: &LULDAuctionCollar) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_operational_halt(&mut self, _msg: &OperationalHalt) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_add_order_no_mpid_attribution(
        &mut self,
        _msg: &AddOrderNoMPIDAttribution,
    ) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_add_order_with_mpid_attribution(
        &mut self,
        _msg: &AddOrderWithMPIDAttribution,
    ) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_order_executed_message(&mut self, _msg: &OrderExecutedMessage) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_order_executed_with_price_message(
        &mut self,
        _msg: &OrderExecutedWithPriceMessage,
    ) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_order_cancel_message(&mut self, _msg: &OrderCancelMessage) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_order_delete_message(&mut self, _msg: &OrderDeleteMessage) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_order_replace_message(&mut self, _msg: &OrderReplaceMessage) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_trade_message(&mut self, _msg: &TradeMessage) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_cross_trade_message(&mut self, _msg: &CrossTradeMessage) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_broken_trade_message(&mut self, _msg: &BrokenTradeMessage) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_net_order_imbalance_indicator_message(
        &mut self,
        _msg: &NetOrderImbalanceIndicatorMessage,
    ) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_retail_price_improvement_indicator(
        &mut self,
        _msg: &RetailPriceImprovementIndicator,
    ) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_direct_listing_with_capital_raise_price_discovery_message(
        &mut self,
        _msg: &DirectListingwithCapitalRaisePriceDiscoveryMessage,
    ) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
}

/// Streaming ITCH 5.0 parser over a byte slice.
///
/// Borrows its input for zero-copy parsing; no heap allocation occurs.
pub struct Parser<'a> {
    buf: &'a [u8],
}

impl<'a> Parser<'a> {
    /// Create a parser over `buf`.
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    /// Iterate over every framed message in the buffer, dispatching each to
    /// the corresponding [`MessageHandler`] method.
    ///
    /// Returns `Ok(())` when the buffer is exhausted or the handler returns
    /// `ControlFlow::Break(())`.  Returns `Err` on any framing or type error.
    pub fn parse_stream(&mut self, handler: &mut impl MessageHandler) -> Result<(), ParseError> {
        while !self.buf.is_empty() {
            let (body, rest) = parse_one(self.buf)?;
            self.buf = rest;
            // body[0] is guaranteed to be a known tag by parse().
            let cf = match body[0] {
                b'S' => handler.on_system_event_message(cast(body)?),
                b'R' => handler.on_stock_directory(cast(body)?),
                b'H' => handler.on_stock_trading_action(cast(body)?),
                b'Y' => handler.on_reg_sho_restriction(cast(body)?),
                b'L' => handler.on_market_participant_position(cast(body)?),
                b'V' => handler.on_mwcb_decline_level_message(cast(body)?),
                b'W' => handler.on_mwcb_status_message(cast(body)?),
                b'K' => handler.on_quoting_period_update(cast(body)?),
                b'J' => handler.on_luld_auction_collar(cast(body)?),
                b'h' => handler.on_operational_halt(cast(body)?),
                b'A' => handler.on_add_order_no_mpid_attribution(cast(body)?),
                b'F' => handler.on_add_order_with_mpid_attribution(cast(body)?),
                b'E' => handler.on_order_executed_message(cast(body)?),
                b'C' => handler.on_order_executed_with_price_message(cast(body)?),
                b'X' => handler.on_order_cancel_message(cast(body)?),
                b'D' => handler.on_order_delete_message(cast(body)?),
                b'U' => handler.on_order_replace_message(cast(body)?),
                b'P' => handler.on_trade_message(cast(body)?),
                b'Q' => handler.on_cross_trade_message(cast(body)?),
                b'B' => handler.on_broken_trade_message(cast(body)?),
                b'I' => handler.on_net_order_imbalance_indicator_message(cast(body)?),
                b'N' => handler.on_retail_price_improvement_indicator(cast(body)?),
                b'O' => handler
                    .on_direct_listing_with_capital_raise_price_discovery_message(cast(body)?),
                unknown => return Err(ParseError::UnknownMessageType(unknown)),
            };
            if cf.is_break() {
                break;
            }
        }
        Ok(())
    }
}

/// Parse a single ITCH 5.0 framed message from `buf`.
///
/// Returns the message body (the bytes after the 2-byte length prefix,
/// starting with the message-type tag) and the remainder of the buffer.
/// Use this when you only need the raw bytes — e.g. to forward them to
/// another consumer — and don't care about the typed contents.
///
/// The tag byte is validated so that framing errors surface immediately
/// rather than silently propagating garbage downstream.
#[inline]
pub fn parse_one(buf: &[u8]) -> Result<(&[u8], &[u8]), ParseError> {
    let (len_bytes, rest) = buf.split_at_checked(2).ok_or(ParseError::EmptyBuffer)?;
    let msg_len = u16::from_be_bytes(len_bytes.try_into().unwrap()) as usize;
    let (body, rest) = rest
        .split_at_checked(msg_len)
        .ok_or(ParseError::MalformedData)?;
    match body.first() {
        Some(
            b'S' | b'R' | b'H' | b'Y' | b'L' | b'V' | b'W' | b'K' | b'J' | b'h' | b'A' | b'F'
            | b'E' | b'C' | b'X' | b'D' | b'U' | b'P' | b'Q' | b'B' | b'I' | b'N' | b'O',
        ) => Ok((body, rest)),
        Some(&unknown) => Err(ParseError::UnknownMessageType(unknown)),
        None => Err(ParseError::MalformedData),
    }
}

/// Attempts to cast bytes as message of type `T`.
/// Generally callers should prefer [`Parser::parse_stream`] or [`parse_one`] over this.
///
/// Note:
/// - `body` should begin with the message's appropriate tag.
/// - `body` should not contain the length prefix.
#[inline]
pub fn cast<T: FromBytes + KnownLayout + Immutable>(body: &[u8]) -> Result<&T, ParseError> {
    T::ref_from_bytes(body).map_err(|_| ParseError::MalformedData)
}

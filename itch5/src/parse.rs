use crate::error::ParseError;
use crate::messages::*;
use std::ops::ControlFlow;
use zerocopy::{FromBytes, Immutable, KnownLayout};

/// Visitor called by [`Parser`] for each decoded ITCH 5.0 message.
///
/// Every method has a default no-op implementation that returns
/// `ControlFlow::Continue(())`, so you only need to override the message
/// types you care about.  Return `ControlFlow::Break(())` from any method
/// to stop parsing immediately.
pub trait MessageHandler {
    fn on_system_event(&mut self, _msg: &SystemEvent) -> ControlFlow<()> {
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
    fn on_mwcb_decline_level(&mut self, _msg: &MWCBDeclineLevel) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_mwcb_status(&mut self, _msg: &MWCBStatus) -> ControlFlow<()> {
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
    fn on_order_executed(&mut self, _msg: &OrderExecuted) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_order_executed_with_price(&mut self, _msg: &OrderExecutedWithPrice) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_order_cancel(&mut self, _msg: &OrderCancel) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_order_delete(&mut self, _msg: &OrderDelete) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_order_replace(&mut self, _msg: &OrderReplace) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_trade(&mut self, _msg: &Trade) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_cross_trade(&mut self, _msg: &CrossTrade) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_broken_trade(&mut self, _msg: &BrokenTrade) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_net_order_imbalance_indicator(
        &mut self,
        _msg: &NetOrderImbalanceIndicator,
    ) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_retail_price_improvement_indicator(
        &mut self,
        _msg: &RetailPriceImprovementIndicator,
    ) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
    fn on_direct_listing_with_capital_raise_price_discovery(
        &mut self,
        _msg: &DirectListingwithCapitalRaisePriceDiscovery,
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
    #[inline]
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    #[inline]
    pub const fn set_buf(&mut self, buf: &'a [u8]) {
        self.buf = buf
    }

    #[inline]
    pub const fn empty() -> Self {
        Self { buf: &[] }
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
                b'S' => handler.on_system_event(cast(body)?),
                b'R' => handler.on_stock_directory(cast(body)?),
                b'H' => handler.on_stock_trading_action(cast(body)?),
                b'Y' => handler.on_reg_sho_restriction(cast(body)?),
                b'L' => handler.on_market_participant_position(cast(body)?),
                b'V' => handler.on_mwcb_decline_level(cast(body)?),
                b'W' => handler.on_mwcb_status(cast(body)?),
                b'K' => handler.on_quoting_period_update(cast(body)?),
                b'J' => handler.on_luld_auction_collar(cast(body)?),
                b'h' => handler.on_operational_halt(cast(body)?),
                b'A' => handler.on_add_order_no_mpid_attribution(cast(body)?),
                b'F' => handler.on_add_order_with_mpid_attribution(cast(body)?),
                b'E' => handler.on_order_executed(cast(body)?),
                b'C' => handler.on_order_executed_with_price(cast(body)?),
                b'X' => handler.on_order_cancel(cast(body)?),
                b'D' => handler.on_order_delete(cast(body)?),
                b'U' => handler.on_order_replace(cast(body)?),
                b'P' => handler.on_trade(cast(body)?),
                b'Q' => handler.on_cross_trade(cast(body)?),
                b'B' => handler.on_broken_trade(cast(body)?),
                b'I' => handler.on_net_order_imbalance_indicator(cast(body)?),
                b'N' => handler.on_retail_price_improvement_indicator(cast(body)?),
                b'O' => handler.on_direct_listing_with_capital_raise_price_discovery(cast(body)?),
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

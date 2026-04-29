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

pub struct Parser<'a> {
    buf: &'a [u8],
}

impl<'a> Parser<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }
    pub fn parse_stream(&mut self, handler: &mut impl MessageHandler) -> Result<(), ParseError> {
        while !self.buf.is_empty() {
            let (len_bytes, rest) = self
                .buf
                .split_at_checked(2)
                .ok_or(ParseError::EmptyBuffer)?;
            let msg_len = u16::from_be_bytes(len_bytes.try_into().unwrap()) as usize;
            let tag = rest.first().ok_or(ParseError::EmptyBuffer)?;
            let (cf, next) = match tag {
                b'S' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_system_event_message(&m), rem)),
                b'R' => {
                    try_parse(rest, msg_len).map(|(m, rem)| (handler.on_stock_directory(&m), rem))
                }
                b'H' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_stock_trading_action(&m), rem)),
                b'Y' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_reg_sho_restriction(&m), rem)),
                b'L' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_market_participant_position(&m), rem)),
                b'V' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_mwcb_decline_level_message(&m), rem)),
                b'W' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_mwcb_status_message(&m), rem)),
                b'K' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_quoting_period_update(&m), rem)),
                b'J' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_luld_auction_collar(&m), rem)),
                b'h' => {
                    try_parse(rest, msg_len).map(|(m, rem)| (handler.on_operational_halt(&m), rem))
                }
                b'A' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_add_order_no_mpid_attribution(&m), rem)),
                b'F' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_add_order_with_mpid_attribution(&m), rem)),
                b'E' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_order_executed_message(&m), rem)),
                b'C' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_order_executed_with_price_message(&m), rem)),
                b'X' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_order_cancel_message(&m), rem)),
                b'D' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_order_delete_message(&m), rem)),
                b'U' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_order_replace_message(&m), rem)),
                b'P' => {
                    try_parse(rest, msg_len).map(|(m, rem)| (handler.on_trade_message(&m), rem))
                }
                b'Q' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_cross_trade_message(&m), rem)),
                b'B' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_broken_trade_message(&m), rem)),
                b'I' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_net_order_imbalance_indicator_message(&m), rem)),
                b'N' => try_parse(rest, msg_len)
                    .map(|(m, rem)| (handler.on_retail_price_improvement_indicator(&m), rem)),
                b'O' => try_parse(rest, msg_len).map(|(m, rem)| {
                    (
                        handler.on_direct_listing_with_capital_raise_price_discovery_message(&m),
                        rem,
                    )
                }),
                unknown => Err(ParseError::UnknownMessageType(*unknown)),
            }?;
            self.buf = next;
            if cf.is_break() {
                break;
            }
        }
        Ok(())
    }
}

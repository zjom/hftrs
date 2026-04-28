use crate::*;
pub trait Visitor {
    fn visit_system_event_message(&mut self, _msg: &SystemEventMessage) {}
    fn visit_stock_directory(&mut self, _msg: &StockDirectory) {}
    fn visit_stock_trading_action(&mut self, _msg: &StockTradingAction) {}
    fn visit_reg_sho_restriction(&mut self, _msg: &RegSHORestriction) {}
    fn visit_market_participant_position(&mut self, _msg: &MarketParticipantPosition) {}
    fn visit_mwcb_decline_level_message(&mut self, _msg: &MWCBDeclineLevelMessage) {}
    fn visit_mwcb_status_message(&mut self, _msg: &MWCBStatusMessage) {}
    fn visit_quoting_period_update(&mut self, _msg: &QuotingPeriodUpdate) {}
    fn visit_luld_auction_collar(&mut self, _msg: &LULDAuctionCollar) {}
    fn visit_operational_halt(&mut self, _msg: &OperationalHalt) {}
    fn visit_add_order_no_mpid_attribution(&mut self, _msg: &AddOrderNoMPIDAttribution) {}
    fn visit_add_order_with_mpid_attribution(&mut self, _msg: &AddOrderWithMPIDAttribution) {}
    fn visit_order_executed_message(&mut self, _msg: &OrderExecutedMessage) {}
    fn visit_order_executed_with_price_message(&mut self, _msg: &OrderExecutedWithPriceMessage) {}
    fn visit_order_cancel_message(&mut self, _msg: &OrderCancelMessage) {}
    fn visit_order_delete_message(&mut self, _msg: &OrderDeleteMessage) {}
    fn visit_order_replace_message(&mut self, _msg: &OrderReplaceMessage) {}
    fn visit_trade_message(&mut self, _msg: &TradeMessage) {}
    fn visit_cross_trade_message(&mut self, _msg: &CrossTradeMessage) {}
    fn visit_broken_trade_message(&mut self, _msg: &BrokenTradeMessage) {}
    fn visit_net_order_imbalance_indicator_message(
        &mut self,
        _msg: &NetOrderImbalanceIndicatorMessage,
    ) {
    }
    fn visit_retail_price_improvement_indicator(&mut self, _msg: &RetailPriceImprovementIndicator) {
    }
    fn visit_direct_listing_with_capital_raise_price_discovery_message(
        &mut self,
        _msg: &DirectListingwithCapitalRaisePriceDiscoveryMessage,
    ) {
    }
}

impl Message<'_> {
    pub fn accept(&self, visitor: &mut impl Visitor) {
        match self {
            Message::SystemEventMessage(msg) => visitor.visit_system_event_message(msg),
            Message::StockDirectory(msg) => visitor.visit_stock_directory(msg),
            Message::StockTradingAction(msg) => visitor.visit_stock_trading_action(msg),
            Message::RegSHORestriction(msg) => visitor.visit_reg_sho_restriction(msg),
            Message::MarketParticipantPosition(msg) => {
                visitor.visit_market_participant_position(msg)
            }
            Message::MWCBDeclineLevelMessage(msg) => visitor.visit_mwcb_decline_level_message(msg),
            Message::MWCBStatusMessage(msg) => visitor.visit_mwcb_status_message(msg),
            Message::QuotingPeriodUpdate(msg) => visitor.visit_quoting_period_update(msg),
            Message::LULDAuctionCollar(msg) => visitor.visit_luld_auction_collar(msg),
            Message::OperationalHalt(msg) => visitor.visit_operational_halt(msg),
            Message::AddOrderNoMPIDAttribution(msg) => {
                visitor.visit_add_order_no_mpid_attribution(msg)
            }
            Message::AddOrderWithMPIDAttribution(msg) => {
                visitor.visit_add_order_with_mpid_attribution(msg)
            }
            Message::OrderExecutedMessage(msg) => visitor.visit_order_executed_message(msg),
            Message::OrderExecutedWithPriceMessage(msg) => {
                visitor.visit_order_executed_with_price_message(msg)
            }
            Message::OrderCancelMessage(msg) => visitor.visit_order_cancel_message(msg),
            Message::OrderDeleteMessage(msg) => visitor.visit_order_delete_message(msg),
            Message::OrderReplaceMessage(msg) => visitor.visit_order_replace_message(msg),
            Message::TradeMessage(msg) => visitor.visit_trade_message(msg),
            Message::CrossTradeMessage(msg) => visitor.visit_cross_trade_message(msg),
            Message::BrokenTradeMessage(msg) => visitor.visit_broken_trade_message(msg),
            Message::NetOrderImbalanceIndicatorMessage(msg) => {
                visitor.visit_net_order_imbalance_indicator_message(msg)
            }
            Message::RetailPriceImprovementIndicator(msg) => {
                visitor.visit_retail_price_improvement_indicator(msg)
            }
            Message::DirectListingwithCapitalRaisePriceDiscoveryMessage(msg) => {
                visitor.visit_direct_listing_with_capital_raise_price_discovery_message(msg)
            }
        }
    }
}

pub struct Messages<'a> {
    buf: &'a [u8],
}

impl<'a> Messages<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    pub fn visit_all(mut self, visitor: &mut impl Visitor) {
        while !self.buf.is_empty() {
            let (msg, rest) = parse(self.buf);
            if let Some(msg) = msg {
                msg.accept(visitor);
            }
            self.buf = rest;
        }
    }
}

impl<'a> Iterator for Messages<'a> {
    type Item = Message<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.buf.is_empty() {
            let (msg, rest) = parse(self.buf);
            self.buf = rest;
            if msg.is_some() {
                return msg;
            }
        }
        None
    }
}

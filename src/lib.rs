mod iter;
mod messages;
mod parse;

pub use iter::*;
pub use messages::*;
pub use parse::parse;

pub enum Message<'a> {
    /// System Event Message
    /// The system event message type is used to signal a market or data feed handler event.
    SystemEventMessage(&'a SystemEventMessage),

    /// Stock Directory
    /// At the start of each trading day, Nasdaq disseminates stock directory messages for all active symbols in the Nasdaq
    /// execution system.
    StockDirectory(&'a StockDirectory),

    /// Stock Trading Action
    /// Nasdaq uses this administrative message to indicate the current trading status of a security to the trading
    /// community.
    StockTradingAction(&'a StockTradingAction),

    /// Reg SHO Short Sale Price Test Restricted Indicator
    RegSHORestriction(&'a RegSHORestriction),

    /// Market Participant Position
    MarketParticipantPosition(&'a MarketParticipantPosition),

    /// Market-Wide Circuit Breaker (MWCB) Decline Level Message
    MWCBDeclineLevelMessage(&'a MWCBDeclineLevelMessage),

    /// Market-Wide Circuit Breaker (MWCB) Status Message
    MWCBStatusMessage(&'a MWCBStatusMessage),

    /// Indicates the anticipated IPO quotation release time of a security.
    QuotingPeriodUpdate(&'a QuotingPeriodUpdate),

    /// Limit Up – Limit Down (LULD) Auction Collar
    LULDAuctionCollar(&'a LULDAuctionCollar),

    /// Operational Halt
    OperationalHalt(&'a OperationalHalt),

    /// Add Order - No MPID Attribution
    AddOrderNoMPIDAttribution(&'a AddOrderNoMPIDAttribution),

    /// Add Order - MPID Attribution
    AddOrderWithMPIDAttribution(&'a AddOrderWithMPIDAttribution),

    /// Order Executed Message
    OrderExecutedMessage(&'a OrderExecutedMessage),

    /// Order Executed With Price Message
    OrderExecutedWithPriceMessage(&'a OrderExecutedWithPriceMessage),

    /// Order Cancel Message
    OrderCancelMessage(&'a OrderCancelMessage),

    /// Order Delete Message
    OrderDeleteMessage(&'a OrderDeleteMessage),

    /// Order Replace Message
    OrderReplaceMessage(&'a OrderReplaceMessage),

    /// Trade Message (Non-Cross)
    TradeMessage(&'a TradeMessage),

    /// Cross Trade Message
    CrossTradeMessage(&'a CrossTradeMessage),

    /// Broken Trade / Order Execution Message
    BrokenTradeMessage(&'a BrokenTradeMessage),

    /// Net Order Imbalance Indicator (NOII) Message
    NetOrderImbalanceIndicatorMessage(&'a NetOrderImbalanceIndicatorMessage),

    /// Retail Price Improvement Indicator (RPII)
    RetailPriceImprovementIndicator(&'a RetailPriceImprovementIndicator),

    /// Direct Listing with Capital Raise Price Discovery Message
    DirectListingwithCapitalRaisePriceDiscoveryMessage(
        &'a DirectListingwithCapitalRaisePriceDiscoveryMessage,
    ),
}

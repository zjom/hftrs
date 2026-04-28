use itch5_derive::itch_message;

pub enum Message<'a> {
    /// System Event Message
    /// The system event message type is used to signal a market or data feed handler event.
    SystemEventMessage(SystemEventMessage<'a>),

    /// Stock Directory
    /// At the start of each trading day, Nasdaq disseminates stock directory messages for all active symbols in the Nasdaq
    /// execution system.
    /// Market data redistributors should process this message to populate the Financial Status Indicator (required display
    /// field) and the Market Category (recommended display field) for Nasdaq listed issues.
    StockDirectory(StockDirectory<'a>),

    /// Stock Trading Action
    /// Nasdaq uses this administrative message to indicate the current trading status of a security to the trading
    /// community.
    /// Prior to the start of system hours, Nasdaq will send out a Trading Action spin. In the spin, Nasdaq will send out a
    /// Stock Trading Action message with the “T” (Trading Resumption) for all Nasdaq--- and other exchange-•-listed
    /// securities that are eligible for trading at the start of the system hours. If a security is absent from the pre-•-
    /// opening Trading Action spin, firms should assume that the security is being treated as halted in the Nasdaq
    /// platform at the start of the system hours. Please note that securities may be halted in the Nasdaq system for
    /// regulatory or operational reasons.
    /// After the start of system hours, Nasdaq will use the Trading Action message to relay changes in trading status for an
    /// individual security. Messages will be sent when a stock is:
    /// • Halted
    /// • Paused*
    /// • Released for quotation
    /// • Released for trading
    /// * The paused status will be disseminated for NASDAQ---listed securities only. Trading pauses on non---NASDAQ listed securities
    /// will be treated simply as a halt.
    StockTradingAction(StockTradingAction<'a>),

    /// Reg SHO Short Sale Price Test Restricted Indicator
    /// In February 2011, the Securities and Exchange Commission (SEC) implemented changes to Rule 201 of the
    /// Regulation SHO (Reg SHO). For details, please refer to SEC Release Number 34-61595. In association with
    /// the Reg SHO rule change, Nasdaq will introduce the following Reg SHO Short Sale Price Test Restricted
    /// Indicator message format.
    /// For Nasdaq-•-listed issues, Nasdaq supports a full pre-•-opening spin of Reg SHO Short Sale Price Test Restricted
    /// Indicator messages indicating the Rule 201 status for all active issues. Nasdaq also sends the Reg SHO
    /// Short Sale Price Test Restricted Indicator message in the event of an intraday status change.
    /// For other exchange-•-listed issues, Nasdaq relays the Reg SHO Short Sale Price Test Restricted Indicator
    /// message when it receives an update from the primary listing exchange.
    /// Nasdaq processes orders based on the most Reg SHO Restriction status value.
    RegSHORestriction(RegSHORestriction<'a>),

    /// Market Participant Position
    /// At the start of each trading day, Nasdaq disseminates a spin of market participant position messages. The
    /// message provides the Primary Market Maker status, Market Maker mode and Market Participant state for
    /// each Nasdaq market participant firm registered in an issue. Market participant firms may use these fields to
    /// comply with certain marketplace rules.
    /// Throughout the day, Nasdaq will send out this message only if Nasdaq Operations changes the status of a
    /// market participant firm in an issue.
    MarketParticipantPosition(MarketParticipantPosition<'a>),

    /// Market-Wide Circuit Breaker (MWCB) Decline Level Message
    /// Informs data recipients what the daily MWCB breach points are set to for the current trading day.
    MWCBDeclineLevelMessage(MWCBDeclineLevelMessage<'a>),

    /// Market-Wide Circuit Breaker (MWCB) Status Message
    /// Informs data recipients when a MWCB has breached one of the established levels
    MWCBStatusMessage(MWCBStatusMessage<'a>),

    /// Indicates the anticipated IPO quotation release time of a security.
    QuotingPeriodUpdate(QuotingPeriodUpdate<'a>),

    /// Limit Up – Limit Down (LULD) Auction Collar
    /// Indicates the auction collar thresholds within which a paused security can reopen following a LULD Trading Pause.
    LULDAuctionCollar(LULDAuctionCollar<'a>),

    /// The Exchange uses this message to indicate the current Operational Status of a security to the trading
    /// community. An Operational Halt means that there has been an interruption of service on the identified
    /// security impacting only the designated Market Center. These Halts differ from the “Stock Trading
    /// Action” message types since an Operational Halt is specific to the exchange for which it is declared, and
    /// does not interrupt the ability of the trading community to trade the identified instrument on any other
    /// marketplace.
    /// Nasdaq uses this administrative message to indicate the current trading status of the three market centers
    /// operated by Nasdaq.
    OperationalHalt(OperationalHalt<'a>),

    /// Add Order - No MPID Attribution
    /// This message will be generated for unattributed orders accepted by the Nasdaq system. (Note: If a firm wants to
    /// display a MPID for unattributed orders, Nasdaq recommends that it use the MPID of “NSDQ”.)
    AddOrderNoMPIDAttribution(AddOrderNoMPIDAttribution<'a>),

    /// This message will be generated for attributed orders and quotations accepted by the Nasdaq system.
    AddOrderWithMPIDAttribution(AddOrderWithMPIDAttribution<'a>),

    /// Order Executed Message
    /// This message is sent whenever an order on the book is executed in whole or in part. It is possible to receive several
    /// Order Executed Messages for the same order reference number if that order is executed in several parts. The
    /// multiple Order Executed Messages on the same order are cumulative.
    /// By combining the executions from both types of Order Executed Messages and the Trade Message, it is possible to
    /// build a complete view of all non-•-cross executions that happen on Nasdaq. Cross execution information is available in
    /// one bulk print per symbol via the Cross Trade Message.
    OrderExecutedMessage(OrderExecutedMessage<'a>),

    /// Order Executed With Price Message
    /// This message is sent whenever an order on the book is executed in whole or in part at a price different from the
    /// initial display price. Since the execution price is different than the display price of the original Add Order, Nasdaq
    /// includes a price field within this execution message.
    /// It is possible to receive multiple Order Executed and Order Executed With Price messages for the same order if that
    /// order is executed in several parts. The multiple Order Executed messages on the same order are cumulative.
    /// These executions may be marked as non-•-printable. If the execution is marked as non-•-printed, it means that the
    /// shares will be included into a later bulk print (e.g., in the case of cross executions). If a firm is looking to use the data
    /// in time-•-and-•-sales displays or volume calculations, Nasdaq recommends that firms ignore messages marked as non-
    /// -- printable to prevent double counting.
    OrderExecutedWithPriceMessage(OrderExecutedWithPriceMessage<'a>),

    /// Order Cancel Message
    /// This message is sent whenever an order on the book is modified as a result of a partial cancellation.
    OrderCancelMessage(OrderCancelMessage<'a>),

    /// Order Delete Message
    /// This message is sent whenever an order on the book is being cancelled. All remaining shares are no longer
    /// accessible so the order must be removed from the book.
    OrderDeleteMessage(OrderDeleteMessage<'a>),

    /// Order Replace Message
    /// This message is sent whenever an order on the book has been cancel-•-replaced. All remaining shares from the
    /// original order are no longer accessible, and must be removed. The new order details are provided for the
    /// replacement, along with a new order reference number which will be used henceforth. Since the side, stock
    /// symbol and attribution (if any) cannot be changed by an Order Replace event, these fields are not included in the
    /// message. Firms should retain the side, stock symbol and MPID from the original Add Order message.
    OrderReplaceMessage(OrderReplaceMessage<'a>),

    /// Trade Message (Non-Cross)
    /// The Trade Message is designed to provide execution details for normal match events involving non-•-displayable
    /// order types. (Note: There is a separate message for Nasdaq cross events.)
    /// Since no Add Order Message is generated when a non-•-displayed order is initially received, Nasdaq cannot use the
    /// Order Executed messages for all matches. Therefore this message indicates when a match occurs between non---
    /// displayable order types. A Trade Message is transmitted each time a non-•-displayable order is executed in whole or
    /// in part. It is possible to receive multiple Trade Messages for the same order if that order is executed in several parts.
    /// Trade Messages for the same order are cumulative.
    /// Trade Messages should be included in Nasdaq time-•-and-•-sales displays as well as volume and other market
    /// statistics. Since Trade Messages do not affect the book, however, they may be ignored by firms just looking to build
    /// and track the Nasdaq execution system display.
    TradeMessage(TradeMessage<'a>),

    /// Cross Trade message indicates that Nasdaq has completed its cross process for a specific security. Nasdaq sends out
    /// a Cross Trade message for all active issues in the system following the Opening, Closing and EMC cross events. Firms
    /// may use the Cross Trade message to determine when the cross for each security has been completed. (Note: For
    /// the halted / paused securities, firms should use the Trading Action message to determine when an issue has been
    /// released for trading.)
    /// For most issues, the Cross Trade message will indicate the bulk volume associated with the cross event. If the order
    /// interest is insufficient to conduct a cross in a particular issue, however, the Cross Trade message may show the
    /// shares as zero.
    /// To avoid double counting of cross volume, firms should not include transactions marked as non-•-printable in time---
    /// and-•-sales displays or market statistic calculations.
    CrossTradeMessage(CrossTradeMessage<'a>),

    /// Broken Trade / Order Execution Message
    /// The Broken Trade Message is sent whenever an execution on Nasdaq is broken. An execution may be broken if it is
    /// found to be “clearly erroneous” pursuant to Nasdaq’s Clearly Erroneous Policy. A trade break is final; once a trade is
    /// broken, it cannot be reinstated.
    /// Firms that use the ITCH feed to create time---and---sales displays or calculate market statistics should be prepared
    /// to process the broken trade message. If a firm is only using the ITCH feed to build a book, however, it may ignore
    /// these messages as they have no impact on the current book.
    BrokenTradeMessage(BrokenTradeMessage<'a>),

    /// Net Order Imbalance Indicator (NOII) Message
    /// • Nasdaq begins disseminating Net Order Imbalance Indicators (NOII) at 9:25 a.m. for the Opening Cross and
    /// 3:50 p.m. for the Closing Cross.
    /// • Between 9:25 and 9:28 a.m. and 3:50 and 3:55 p.m., Nasdaq disseminates the NOII information every 10
    /// seconds.
    /// • Between 9:28 and 9:30 a.m. and 3:55 and 4:00 p.m., Nasdaq disseminates the NOII information every
    /// second.
    /// • For Nasdaq Halt, IPO and Pauses, NOII messages will be disseminated at 1 second intervals starting 1
    /// second after quoting period starts/trading action is released.
    /// • For more information, please see the FAQ on Opening and Closing Crosses.
    /// • Nasdaq will also disseminate an Extended Trading Close (ETC) message from 4:00 p.m. to 4:05 p.m. at five
    /// second intervals.
    /// • For more information, please see the FAQ on Extended Trading Close.
    NetOrderImbalanceIndicatorMessage(NetOrderImbalanceIndicatorMessage<'a>),

    /// Retail Price Improvement Indicator (RPII)
    /// Identifies a retail interest indication of the Bid, Ask or both the Bid and Ask for Nasdaq-•-listed securities.
    RetailPriceImprovementIndicator(RetailPriceImprovementIndicator<'a>),

    /// Direct Listing with Capital Raise Price Discovery Message
    /// The following message is disseminated only for Direct Listing with Capital Raise (DLCR) securities. Nasdaq begins
    /// disseminating messages once per second as soon as the DLCR volatility test has successfully passed.
    DirectListingwithCapitalRaisePriceDiscoveryMessage(
        DirectListingwithCapitalRaisePriceDiscoveryMessage<'a>,
    ),
}

pub fn parse<'a>(buf: &'a [u8]) -> (Option<Message<'a>>, &'a [u8]) {
    match buf[0] {
        b'S' => (
            SystemEventMessage::parse(buf),
            buf.split_at(SystemEventMessage::LEN).1,
        ),
        b'R' => (
            StockDirectory::parse(buf),
            buf.split_at(StockDirectory::LEN).1,
        ),
        b'H' => (
            StockTradingAction::parse(buf),
            buf.split_at(StockTradingAction::LEN).1,
        ),
        b'Y' => (
            RegSHORestriction::parse(buf),
            buf.split_at(RegSHORestriction::LEN).1,
        ),
        b'L' => (
            MarketParticipantPosition::parse(buf),
            buf.split_at(MarketParticipantPosition::LEN).1,
        ),
        b'V' => (
            MWCBDeclineLevelMessage::parse(buf),
            buf.split_at(MWCBDeclineLevelMessage::LEN).1,
        ),
        b'W' => (
            MWCBStatusMessage::parse(buf),
            buf.split_at(MWCBStatusMessage::LEN).1,
        ),
        b'K' => (
            QuotingPeriodUpdate::parse(buf),
            buf.split_at(QuotingPeriodUpdate::LEN).1,
        ),
        b'J' => (
            LULDAuctionCollar::parse(buf),
            buf.split_at(LULDAuctionCollar::LEN).1,
        ),
        b'h' => (
            OperationalHalt::parse(buf),
            buf.split_at(OperationalHalt::LEN).1,
        ),
        b'A' => (
            AddOrderNoMPIDAttribution::parse(buf),
            buf.split_at(AddOrderNoMPIDAttribution::LEN).1,
        ),
        b'F' => (
            AddOrderWithMPIDAttribution::parse(buf),
            buf.split_at(AddOrderWithMPIDAttribution::LEN).1,
        ),
        b'E' => (
            OrderExecutedMessage::parse(buf),
            buf.split_at(OrderExecutedMessage::LEN).1,
        ),
        b'C' => (
            OrderExecutedWithPriceMessage::parse(buf),
            buf.split_at(OrderExecutedWithPriceMessage::LEN).1,
        ),
        b'X' => (
            OrderCancelMessage::parse(buf),
            buf.split_at(OrderCancelMessage::LEN).1,
        ),
        b'D' => (
            OrderDeleteMessage::parse(buf),
            buf.split_at(OrderDeleteMessage::LEN).1,
        ),
        b'U' => (
            OrderReplaceMessage::parse(buf),
            buf.split_at(OrderReplaceMessage::LEN).1,
        ),
        b'P' => (TradeMessage::parse(buf), buf.split_at(TradeMessage::LEN).1),
        b'Q' => (
            CrossTradeMessage::parse(buf),
            buf.split_at(CrossTradeMessage::LEN).1,
        ),
        b'B' => (
            BrokenTradeMessage::parse(buf),
            buf.split_at(BrokenTradeMessage::LEN).1,
        ),
        b'I' => (
            NetOrderImbalanceIndicatorMessage::parse(buf),
            buf.split_at(NetOrderImbalanceIndicatorMessage::LEN).1,
        ),
        b'N' => (
            RetailPriceImprovementIndicator::parse(buf),
            buf.split_at(RetailPriceImprovementIndicator::LEN).1,
        ),
        b'O' => (
            DirectListingwithCapitalRaisePriceDiscoveryMessage::parse(buf),
            buf.split_at(DirectListingwithCapitalRaisePriceDiscoveryMessage::LEN)
                .1,
        ),
        _ => (None, &buf[1..]),
    }
}

/// Prices are integer fields, supplied with an associated precision. When converted to a decimal format, prices are in
/// fixed point format, where the precision defines the number of decimal places. For example, a field flagged as Price
/// (4) has an implied 4 decimal places. The maximum value of price (4) in TotalView ITCH is 200,000.0000 (decimal,
/// 77359400 hex).
pub struct Price4<'a>(&'a [u8]);
impl Price4<'_> {
    pub fn into_u32(&self) -> u32 {
        u32::from_be_bytes(self.0.try_into().unwrap())
    }

    pub fn into_f64(&self) -> f64 {
        f64::from(self.into_u32()) / 10000.0
    }
}

impl<'a> From<&'a [u8]> for Price4<'a> {
    fn from(value: &'a [u8]) -> Self {
        Self(value)
    }
}

/// Prices are integer fields, supplied with an associated precision. When converted to a decimal format, prices are in
/// fixed point format, where the precision defines the number of decimal places.
pub struct Price8<'a>(&'a [u8]);
impl Price8<'_> {
    pub fn into_u64(&self) -> u64 {
        u64::from_be_bytes(self.0.try_into().unwrap())
    }
    pub fn into_f64(&self) -> f64 {
        self.into_u64() as f64 / 1_0000_0000.0
    }
}
impl<'a> From<&'a [u8]> for Price8<'a> {
    fn from(value: &'a [u8]) -> Self {
        Self(value)
    }
}

/// System Event Message
/// The system event message type is used to signal a market or data feed handler event. The format is as follows:
/// Name Offset Length Value Notes
/// Message Type 0 1 “S” System Event Message
/// Stock Locate 1 2 Integer Always 0
/// Tracking Number 3 2 Integer Nasdaq internal tracking number
/// Timestamp 5 6 Integer Nanoseconds since midnight
/// Event Code 11 1 Alpha See System Event Codes below

#[itch_message(tag = b'S')]
pub struct SystemEventMessage {
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    #[field(offset = 11, len = 1)]
    event_code: SystemEventCode,
}

/// Nasdaq supports the following event codes on a daily basis on the TotalView-ITCH data feed.
/// Code Explanation
/// “O” Start of Messages. Outside of time stamp messages, the start of day message is the first message sent in
/// any trading day.
/// “S” Start of System hours. This message indicates that NASDAQ is open and ready to start accepting orders.
/// “Q” Start of Market hours. This message is intended to indicate that Market Hours orders are available
/// for execution.
/// “M” End of Market hours. This message is intended to indicate that Market Hours orders are no longer
/// available for execution.
/// “E” End of System hours. It indicates that Nasdaq is now closed and will not accept any new orders today.
/// It is still possible to receive Broken Trade messages and Order Delete messages after the End of Day
/// .“C” End of Messages. This is always the last message sent in any trading day.
#[repr(u8)]
pub enum SystemEventCode {
    StartOfMessages = b'O',
    StartOfSystemHours = b'S',
    StartOfMarketHours = b'Q',
    EndOfMarketHours = b'M',
    EndOfSystemHours = b'E',
    EndOfMessages = b'C',

    Unknown(u8),
}

impl SystemEventCode {
    pub fn from_byte(b: u8) -> Self {
        match b {
            b'O' => Self::StartOfMessages,
            b'S' => Self::StartOfSystemHours,
            b'Q' => Self::StartOfMarketHours,
            b'M' => Self::EndOfMarketHours,
            b'E' => Self::EndOfSystemHours,
            b'C' => Self::EndOfMessages,
            unknown => Self::Unknown(unknown),
        }
    }
}
impl From<u8> for SystemEventCode {
    fn from(value: u8) -> Self {
        SystemEventCode::from_byte(value)
    }
}

/// Stock Directory
/// At the start of each trading day, Nasdaq disseminates stock directory messages for all active symbols in the Nasdaq
/// execution system.
/// Market data redistributors should process this message to populate the Financial Status Indicator (required display field) and the Market Category (recommended display field) for Nasdaq listed issues.
#[itch_message(tag = b'R')]
pub struct StockDirectory {
    /// Locate Code uniquely assigned to the security symbol for the day.
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Time at which the directory message was generated. Refer to Data Types for field processing notes.
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// Denotes the security symbol for the issue in the Nasdaq execution system.
    #[field(offset = 11, len = 8)]
    stock: &[u8],
    /// Indicates Listing market or listing market tier for the issue
    #[field(offset = 19, len = 1)]
    market_category: MarketCategory,
    /// For Nasdaq listed issues, this field indicates when a firm is not in compliance with Nasdaq continued listing requirements
    #[field(offset = 20, len = 1)]
    financial_status_indicator: FinancialStatusIndicator,
    /// Denotes the number of shares that represent a round lot for the issue
    #[field(offset = 21, len = 4)]
    round_lot_size: u32,
    /// Indicates if Nasdaq system limits order entry for issue
    #[field(offset = 25, len = 1)]
    round_lots_only: RoundLotsOnly,
    /// Identifies the security class for the issue as assigned by Nasdaq. See Appendix for allowable values.
    #[field(offset = 26, len = 1)]
    issue_classification: u8,
    /// Identifies the security sub-type for the issue as assigned by Nasdaq. See Appendix for allowable values.
    #[field(offset = 27, len = 2)]
    issue_sub_type: &[u8],
    /// Denotes if an issue or quoting participant record is set-up in Nasdaq systems in a live/production, test, or demo state.
    #[field(offset = 29, len = 1)]
    authenticity: Authenticity,
    /// Indicates if a security is subject to mandatory close-out of short sales under SEC Rule 203(b)(3).
    #[field(offset = 30, len = 1)]
    short_sale_threshold_indicator: ShortSaleThresholdIndicator,
    /// Indicates if the Nasdaq security is set up for IPO release.
    #[field(offset = 31, len = 1)]
    ipo_flag: IpoFlag,
    /// Indicates which Limit Up / Limit Down price band calculation parameter is to be used for the instrument.
    #[field(offset = 32, len = 1)]
    luld_reference_price_tier: LuldReferencePriceTier,
    /// Indicates whether the security is an exchange traded product (ETP).
    #[field(offset = 33, len = 1)]
    etp_flag: EtpFlag,
    /// Tracks the integral relationship of the ETP to the underlying index.
    #[field(offset = 34, len = 4)]
    etp_leverage_factor: u32,
    /// Indicates the directional relationship between the ETP and Underlying index.
    #[field(offset = 38, len = 1)]
    inverse_indicator: InverseIndicator,
}

#[repr(u8)]
pub enum MarketCategory {
    NasdaqGlobalSelectMarket = b'Q',
    NasdaqGlobalMarket = b'G',
    NasdaqCapitalMarket = b'S',
    NYSE = b'N',
    NYSEAmerican = b'A',
    NYSEArca = b'P',
    BATSZExchange = b'Z',
    InvestorsExchangeLLC = b'V',
    NotAvailable = b' ',
    Unknown(u8),
}

impl From<u8> for MarketCategory {
    fn from(value: u8) -> Self {
        match value {
            b'Q' => Self::NasdaqGlobalSelectMarket,
            b'G' => Self::NasdaqGlobalMarket,
            b'S' => Self::NasdaqCapitalMarket,
            b'N' => Self::NYSE,
            b'A' => Self::NYSEAmerican,
            b'P' => Self::NYSEArca,
            b'Z' => Self::BATSZExchange,
            b'V' => Self::InvestorsExchangeLLC,
            b' ' => Self::NotAvailable,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum FinancialStatusIndicator {
    Deficient = b'D',
    Delinquent = b'E',
    Bankrupt = b'Q',
    Suspended = b'S',
    DeficientBankrupt = b'G',
    DeficientDelinquent = b'H',
    DelinquentBankrupt = b'J',
    DeficientDelinquentBankrupt = b'K',
    CreationsRedemptionsSuspendedETP = b'C',
    Normal = b'N',
    Unknown(u8),
}

impl From<u8> for FinancialStatusIndicator {
    fn from(value: u8) -> Self {
        match value {
            b'D' => Self::Deficient,
            b'E' => Self::Delinquent,
            b'Q' => Self::Bankrupt,
            b'S' => Self::Suspended,
            b'G' => Self::DeficientBankrupt,
            b'H' => Self::DeficientDelinquent,
            b'J' => Self::DelinquentBankrupt,
            b'K' => Self::DeficientDelinquentBankrupt,
            b'C' => Self::CreationsRedemptionsSuspendedETP,
            b'N' => Self::Normal,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum RoundLotsOnly {
    Yes = b'Y',
    No = b'N',
    Unknown(u8),
}

impl From<u8> for RoundLotsOnly {
    fn from(value: u8) -> Self {
        match value {
            b'Y' => Self::Yes,
            b'N' => Self::No,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum Authenticity {
    LiveProduction = b'P',
    Test = b'T',
    Unknown(u8),
}

impl From<u8> for Authenticity {
    fn from(value: u8) -> Self {
        match value {
            b'P' => Self::LiveProduction,
            b'T' => Self::Test,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum ShortSaleThresholdIndicator {
    Restricted = b'Y',
    NotRestricted = b'N',
    NotAvailable = b' ',
    Unknown(u8),
}

impl From<u8> for ShortSaleThresholdIndicator {
    fn from(value: u8) -> Self {
        match value {
            b'Y' => Self::Restricted,
            b'N' => Self::NotRestricted,
            b' ' => Self::NotAvailable,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum IpoFlag {
    NewIPO = b'Y',
    NotNewIPO = b'N',
    NotAvailable = b' ',
    Unknown(u8),
}

impl From<u8> for IpoFlag {
    fn from(value: u8) -> Self {
        match value {
            b'Y' => Self::NewIPO,
            b'N' => Self::NotNewIPO,
            b' ' => Self::NotAvailable,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum LuldReferencePriceTier {
    Tier1 = b'1',
    Tier2 = b'2',
    NotAvailable = b' ',
    Unknown(u8),
}

impl From<u8> for LuldReferencePriceTier {
    fn from(value: u8) -> Self {
        match value {
            b'1' => Self::Tier1,
            b'2' => Self::Tier2,
            b' ' => Self::NotAvailable,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum EtpFlag {
    IsETP = b'Y',
    NotETP = b'N',
    NotAvailable = b' ',
    Unknown(u8),
}

impl From<u8> for EtpFlag {
    fn from(value: u8) -> Self {
        match value {
            b'Y' => Self::IsETP,
            b'N' => Self::NotETP,
            b' ' => Self::NotAvailable,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum InverseIndicator {
    Inverse = b'Y',
    NotInverse = b'N',
    Unknown(u8),
}

impl From<u8> for InverseIndicator {
    fn from(value: u8) -> Self {
        match value {
            b'Y' => Self::Inverse,
            b'N' => Self::NotInverse,
            unknown => Self::Unknown(unknown),
        }
    }
}

/// Stock Trading Action Message
/// Nasdaq uses this administrative message to indicate the current trading status of a security to the trading
/// community.
/// Prior to the start of system hours, Nasdaq will send out a Trading Action spin. In the spin, Nasdaq will send out a
/// Stock Trading Action message with the “T” (Trading Resumption) for all Nasdaq--- and other exchange-•-listed
/// securities that are eligible for trading at the start of the system hours. If a security is absent from the pre-•-
/// opening Trading Action spin, firms should assume that the security is being treated as halted in the Nasdaq
/// platform at the start of the system hours. Please note that securities may be halted in the Nasdaq system for
/// regulatory or operational reasons.
/// After the start of system hours, Nasdaq will use the Trading Action message to relay changes in trading status for an
/// individual security. Messages will be sent when a stock is:
/// • Halted
/// • Paused*
/// • Released for quotation
/// • Released for trading
/// * The paused status will be disseminated for NASDAQ---listed securities only. Trading pauses on non---NASDAQ listed securities
/// will be treated simply as a halt.
#[itch_message(tag = b'H')]
pub struct StockTradingAction {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// Stock symbol, right padded with spaces
    #[field(offset = 11, len = 8)]
    stock: &[u8],
    /// Indicates the current trading state for the stock.
    #[field(offset = 19, len = 1)]
    trading_state: TradingState,
    /// Reserved.
    #[field(offset = 20, len = 1)]
    reserved: u8,
    /// Trading Action reason.
    #[field(offset = 21, len = 4)]
    reason: &[u8],
}

#[repr(u8)]
pub enum TradingState {
    /// Halted across all U.S. equity markets / SROs
    Halted = b'H',
    /// Paused across all U.S. equity markets / SROs (Nasdaq-listed securities only)
    Paused = b'P',
    /// Quotation only period for cross-SRO halt or pause
    QuotationOnly = b'Q',
    /// Trading on Nasdaq
    Trading = b'T',
    Unknown(u8),
}

impl From<u8> for TradingState {
    fn from(value: u8) -> Self {
        match value {
            b'H' => Self::Halted,
            b'P' => Self::Paused,
            b'Q' => Self::QuotationOnly,
            b'T' => Self::Trading,
            unknown => Self::Unknown(unknown),
        }
    }
}

/// Reg SHO Short Sale Price Test Restricted Indicator
/// In February 2011, the Securities and Exchange Commission (SEC) implemented changes to Rule 201 of the
/// Regulation SHO (Reg SHO). For details, please refer to SEC Release Number 34-61595. In association with
/// the Reg SHO rule change, Nasdaq will introduce the following Reg SHO Short Sale Price Test Restricted
/// Indicator message format.
/// For Nasdaq-•-listed issues, Nasdaq supports a full pre-•-opening spin of Reg SHO Short Sale Price Test Restricted
/// Indicator messages indicating the Rule 201 status for all active issues. Nasdaq also sends the Reg SHO
/// Short Sale Price Test Restricted Indicator message in the event of an intraday status change.
/// For other exchange-•-listed issues, Nasdaq relays the Reg SHO Short Sale Price Test Restricted Indicator
/// message when it receives an update from the primary listing exchange.
/// Nasdaq processes orders based on the most Reg SHO Restriction status value.
#[itch_message(tag = b'Y')]
pub struct RegSHORestriction {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    locate_code: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// Stock symbol, right padded with spaces
    #[field(offset = 11, len = 8)]
    stock: &[u8],
    /// Denotes the Reg SHO Short Sale Price Test Restriction status for the issue at the time of the message dissemination.
    #[field(offset = 19, len = 1)]
    reg_sho_action: RegShoAction,
}

#[repr(u8)]
pub enum RegShoAction {
    /// No price test in place
    NoPriceTest = b'0',
    /// Reg SHO Short Sale Price Test Restriction in effect due to an intra-day price drop in security
    RestrictionInEffectIntradayDrop = b'1',
    /// Reg SHO Short Sale Price Test Restriction remains in effect
    RestrictionRemainsInEffect = b'2',
    Unknown(u8),
}

impl From<u8> for RegShoAction {
    fn from(value: u8) -> Self {
        match value {
            b'0' => Self::NoPriceTest,
            b'1' => Self::RestrictionInEffectIntradayDrop,
            b'2' => Self::RestrictionRemainsInEffect,
            unknown => Self::Unknown(unknown),
        }
    }
}

/// Market Participant Position message
/// At the start of each trading day, Nasdaq disseminates a spin of market participant position messages. The
/// message provides the Primary Market Maker status, Market Maker mode and Market Participant state for
/// each Nasdaq market participant firm registered in an issue. Market participant firms may use these fields to
/// comply with certain marketplace rules.
/// Throughout the day, Nasdaq will send out this message only if Nasdaq Operations changes the status of a
/// market participant firm in an issue.
#[itch_message(tag = b'L')]
pub struct MarketParticipantPosition {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// Denotes the market participant identifier for which the position message is being generated
    #[field(offset = 11, len = 4)]
    mpid: &[u8],
    /// Stock symbol, right padded with spaces
    #[field(offset = 15, len = 8)]
    stock: &[u8],
    /// Indicates if the market participant firm qualifies as a Primary Market Maker in accordance with Nasdaq marketplace rules
    #[field(offset = 23, len = 1)]
    primary_market_maker: PrimaryMarketMaker,
    /// Indicates the quoting participant’s registration status in relation to SEC Rules 101 and 104 of Regulation M
    #[field(offset = 24, len = 1)]
    market_maker_mode: MarketMakerMode,
    /// Indicates the market participant’s current registration status in the issue
    #[field(offset = 25, len = 1)]
    market_participant_state: MarketParticipantState,
}

#[repr(u8)]
pub enum PrimaryMarketMaker {
    /// primary market maker
    Primary = b'Y',
    /// non-primary market maker
    NonPrimary = b'N',
    Unknown(u8),
}

impl From<u8> for PrimaryMarketMaker {
    fn from(value: u8) -> Self {
        match value {
            b'Y' => Self::Primary,
            b'N' => Self::NonPrimary,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum MarketMakerMode {
    /// normal
    Normal = b'N',
    /// passive
    Passive = b'P',
    /// syndicate
    Syndicate = b'S',
    /// pre-syndicate
    PreSyndicate = b'R',
    /// penalty
    Penalty = b'L',
    Unknown(u8),
}

impl From<u8> for MarketMakerMode {
    fn from(value: u8) -> Self {
        match value {
            b'N' => Self::Normal,
            b'P' => Self::Passive,
            b'S' => Self::Syndicate,
            b'R' => Self::PreSyndicate,
            b'L' => Self::Penalty,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum MarketParticipantState {
    /// Active
    Active = b'A',
    /// Excused/Withdrawn
    Deleted = b'D',
    ExcusedWithdrawn = b'E',
    /// Withdrawn
    Withdrawn = b'W',
    /// Suspended
    Suspended = b'S',
    /// Deleted
    Unknown(u8),
}

impl From<u8> for MarketParticipantState {
    fn from(value: u8) -> Self {
        match value {
            b'A' => Self::Active,
            b'E' => Self::ExcusedWithdrawn,
            b'W' => Self::Withdrawn,
            b'S' => Self::Suspended,
            b'D' => Self::Deleted,
            unknown => Self::Unknown(unknown),
        }
    }
}

/// Market wide circuit breaker Decline Level Message
#[itch_message(tag = b'V')]
pub struct MWCBDeclineLevelMessage {
    /// Always set to 0
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Time at which the MWCB Decline Level message was generated
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// Denotes the MWCB Level 1 Value.
    #[field(offset = 11, len = 8)]
    level_1: Price8,
    /// Denotes the MWCB Level 2 Value.
    #[field(offset = 19, len = 8)]
    level_2: Price8,
    /// Denotes the MWCB Level 3 Value.
    #[field(offset = 27, len = 8)]
    level_3: Price8,
}

/// Market-Wide Circuit Breaker Status message
#[itch_message(tag = b'W')]
pub struct MWCBStatusMessage {
    /// Always set to 0
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Time at which the MWCB Breaker Status message was generated
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// Denotes the MWCB Level that was breached.
    #[field(offset = 11, len = 1)]
    breached_level: BreachedLevel,
}

#[repr(u8)]
pub enum BreachedLevel {
    /// Level 1
    Level1 = b'1',
    /// Level 2
    Level2 = b'2',
    /// Level 3
    Level3 = b'3',
    Unknown(u8),
}

impl From<u8> for BreachedLevel {
    fn from(value: u8) -> Self {
        match value {
            b'1' => Self::Level1,
            b'2' => Self::Level2,
            b'3' => Self::Level3,
            unknown => Self::Unknown(unknown),
        }
    }
}

/// IPO Quoting Period Update Message
/// Indicates the anticipated IPO quotation release time of a security.
#[itch_message(tag = b'K')]
pub struct QuotingPeriodUpdate {
    /// Always set to 0
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Time at which the IPO Quoting Period Update message was generated
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// Stock symbol, right padded with spaces
    #[field(offset = 11, len = 8)]
    stock: &[u8],
    /// Denotes the IPO release time, in seconds since midnight, for quotation to the nearest second.
    /// NOTE: If the quotation period is being canceled/postponed, IPO Quotation Time and IPO Price will be set to 0.
    #[field(offset = 19, len = 4)]
    ipo_quotation_release_time: u32,
    /// Anticipated Quotation Release Time or IPO Release Canceled/Postponed
    #[field(offset = 23, len = 1)]
    ipo_quotation_release_qualifier: IpoQuotationReleaseQualifier,
    /// Denotes the IPO Price to be used for intraday net change calculations.
    /// Prices are given in decimal format with 6 whole number places followed by 4 decimal digits.
    #[field(offset = 24, len = 4)]
    ipo_price: Price4,
}

#[repr(u8)]
pub enum IpoQuotationReleaseQualifier {
    /// Anticipated Quotation Release Time: This value would be used when Nasdaq Market Operations initially enters the IPO instrument for release
    Anticipated = b'A',
    /// IPO Release Canceled/Postponed: This value would be used when Nasdaq Market Operations cancels or postpones the release of the new IPO instrument
    CanceledPostponed = b'C',
    Unknown(u8),
}

impl From<u8> for IpoQuotationReleaseQualifier {
    fn from(value: u8) -> Self {
        match value {
            b'A' => Self::Anticipated,
            b'C' => Self::CanceledPostponed,
            unknown => Self::Unknown(unknown),
        }
    }
}

/// Limit Up – Limit Down (LULD) Auction Collar
/// Indicates the auction collar thresholds within which a paused security can reopen following a LULD Trading pause.
#[itch_message(tag = b'J')]
pub struct LULDAuctionCollar {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds past midnight
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// Stock symbol, right padded with spaces
    #[field(offset = 11, len = 8)]
    stock: &[u8],
    /// Reference price used to set the Auction Collars
    #[field(offset = 19, len = 4)]
    auction_collar_reference_price: Price4,
    /// Indicates the price of the Upper Auction Collar Threshold
    #[field(offset = 23, len = 4)]
    upper_auction_collar_price: Price4,
    /// Indicates the price of the Lower Auction Collar Threshold
    #[field(offset = 27, len = 4)]
    lower_auction_collar_price: Price4,
    /// Indicates the number of the extensions to the Reopening Auction
    #[field(offset = 31, len = 4)]
    auction_collar_extension: Price4,
}

/// Operational Halt Message
/// The Exchange uses this message to indicate the current Operational Status of a security to the trading
/// community. An Operational Halt means that there has been an interruption of service on the identified
/// security impacting only the designated Market Center. These Halts differ from the “Stock Trading
/// Action” message types since an Operational Halt is specific to the exchange for which it is declared, and
/// does not interrupt the ability of the trading community to trade the identified instrument on any other
/// marketplace.
/// Nasdaq uses this administrative message to indicate the current trading status of the three market centers
/// operated by Nasdaq.
#[itch_message(tag = b'h')]
pub struct OperationalHalt {
    /// Locate code uniquely assigned to the security symbol for the day.
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Time at which the Operational Halt message was generated.
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// Denotes the security symbol for the issue in Nasdaq execution system
    #[field(offset = 11, len = 8)]
    stock: &[u8],
    /// Market Code
    #[field(offset = 19, len = 1)]
    market_code: MarketCode,
    /// Operational Halt Action
    #[field(offset = 20, len = 1)]
    operational_halt_action: OperationalHaltAction,
}

#[repr(u8)]
pub enum MarketCode {
    /// Nasdaq
    Nasdaq = b'Q',
    /// BX
    BX = b'B',
    /// PSX
    PSX = b'X',
    Unknown(u8),
}

impl From<u8> for MarketCode {
    fn from(value: u8) -> Self {
        match value {
            b'Q' => Self::Nasdaq,
            b'B' => Self::BX,
            b'X' => Self::PSX,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum OperationalHaltAction {
    /// Operationally Halted on the identified Market
    OperationallyHalted = b'H',
    /// Operational Halt has been lifted and Trading resumed
    TradingResumed = b'T',
    Unknown(u8),
}

impl From<u8> for OperationalHaltAction {
    fn from(value: u8) -> Self {
        match value {
            b'H' => Self::OperationallyHalted,
            b'T' => Self::TradingResumed,
            unknown => Self::Unknown(unknown),
        }
    }
}

/// Add Order – No MPID Attribution Message
/// This message will be generated for unattributed orders accepted by the Nasdaq system. (Note: If a firm wants to display a MPID for unattributed orders, Nasdaq recommends that it use the MPID of “NSDQ”.)
#[itch_message(tag = b'A')]
pub struct AddOrderNoMPIDAttribution {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight.
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// The unique reference number assigned to the new order at the time of receipt.
    #[field(offset = 11, len = 8)]
    order_reference_number: u64,
    /// The type of order being added.
    #[field(offset = 19, len = 1)]
    buy_sell_indicator: BuySellIndicator,
    /// The total number of shares associated with the order being added to the book.
    #[field(offset = 20, len = 4)]
    shares: u32,
    /// Stock symbol, right padded with spaces
    #[field(offset = 24, len = 8)]
    stock: &[u8],
    /// The display price of the new order. Refer to Data Types for field processing notes.
    #[field(offset = 32, len = 4)]
    price: Price4,
}

#[repr(u8)]
pub enum BuySellIndicator {
    /// Buy Order
    Buy = b'B',
    /// Sell Order
    Sell = b'S',
    Unknown(u8),
}

impl From<u8> for BuySellIndicator {
    fn from(value: u8) -> Self {
        match value {
            b'B' => Self::Buy,
            b'S' => Self::Sell,
            unknown => Self::Unknown(unknown),
        }
    }
}

/// Add Order - MPID Attribution Message
/// This message will be generated for attributed orders and quotations accepted by the Nasdaq system.
#[itch_message(tag = b'F')]
pub struct AddOrderWithMPIDAttribution {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight.
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// The unique reference number assigned to the new order at the time of receipt.
    #[field(offset = 11, len = 8)]
    order_reference_number: u64,
    /// The type of order being added.
    #[field(offset = 19, len = 1)]
    buy_sell_indicator: BuySellIndicator,
    /// The total number of shares associated with the order being added to the book
    #[field(offset = 20, len = 4)]
    shares: u32,
    /// Stock symbol, right padded with spaces
    #[field(offset = 24, len = 8)]
    stock: &[u8],
    /// The display price of the new order. Refer to Data Types for field processing notes.
    #[field(offset = 32, len = 4)]
    price: Price4,
    /// Nasdaq Market participant identifier associated with the entered order
    #[field(offset = 36, len = 4)]
    attribution: &[u8],
}

/// Order Executed Message
/// This message is sent whenever an order on the book is executed in whole or in part. It is possible to receive several
/// Order Executed Messages for the same order reference number if that order is executed in several parts. The
/// multiple Order Executed Messages on the same order are cumulative.
/// By combining the executions from both types of Order Executed Messages and the Trade Message, it is possible to
/// build a complete view of all non-•-cross executions that happen on Nasdaq. Cross execution information is available in one bulk print per symbol via the Cross Trade Message.
#[itch_message(tag = b'E')]
pub struct OrderExecutedMessage {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// The unique reference number assigned to the new order at the time of receipt
    #[field(offset = 11, len = 8)]
    order_reference_number: u64,
    /// The number of shares executed
    #[field(offset = 19, len = 4)]
    executed_shares: u32,
    /// The Nasdaq generated day unique Match Number of this execution. The Match Number is also referenced in the Trade Break Message
    #[field(offset = 23, len = 8)]
    match_number: u64,
}

/// Order Executed With Price Message
/// This message is sent whenever an order on the book is executed in whole or in part at a price different from the
/// initial display price. Since the execution price is different than the display price of the original Add Order, Nasdaq
/// includes a price field within this execution message.
/// It is possible to receive multiple Order Executed and Order Executed With Price messages for the same order if that
/// order is executed in several parts. The multiple Order Executed messages on the same order are cumulative.
/// These executions may be marked as non-•-printable. If the execution is marked as non-•-printed, it means that the
/// shares will be included into a later bulk print (e.g., in the case of cross executions). If a firm is looking to use the data
/// in time-•-and-•-sales displays or volume calculations, Nasdaq recommends that firms ignore messages marked as non-
/// -- printable to prevent double counting.
#[itch_message(tag = b'C')]
pub struct OrderExecutedWithPriceMessage {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// The unique reference number assigned to the new order at the time of receipt
    #[field(offset = 11, len = 8)]
    order_reference_number: u64,
    /// The number of shares executed
    #[field(offset = 19, len = 4)]
    executed_shares: u32,
    /// The Nasdaq generated day unique Match Number of this execution. The Match Number is also referenced in the Trade Break Message
    #[field(offset = 23, len = 8)]
    match_number: u64,
    /// Indicates if the execution should be reflected on time and sales displays and volume calculations
    #[field(offset = 31, len = 1)]
    printable: Printable,
    /// The Price at which the order execution occurred. Refer to Data Types for field processing notes
    #[field(offset = 32, len = 4)]
    execution_price: Price4,
}

#[repr(u8)]
pub enum Printable {
    /// Non-Printable
    NonPrintable = b'N',
    /// Printable
    Printable = b'Y',
    Unknown(u8),
}

impl From<u8> for Printable {
    fn from(value: u8) -> Self {
        match value {
            b'N' => Self::NonPrintable,
            b'Y' => Self::Printable,
            unknown => Self::Unknown(unknown),
        }
    }
}

/// Order Cancel Message
/// This message is sent whenever an order on the book is modified as a result of a partial cancellation.
#[itch_message(tag = b'X')]
pub struct OrderCancelMessage {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// The reference number of the order being canceled
    #[field(offset = 11, len = 8)]
    order_reference_number: u64,
    /// The number of shares being removed from the display size of the order as a result of a cancellation
    #[field(offset = 19, len = 4)]
    cancelled_shares: u32,
}

/// Order Delete Message
/// This message is sent whenever an order on the book is being cancelled. All remaining shares are no longer accessible so the order must be removed from the book.
#[itch_message(tag = b'D')]
pub struct OrderDeleteMessage {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// The reference number of the order being canceled
    #[field(offset = 11, len = 8)]
    order_reference_number: u64,
}

/// Order Replace Message
/// This message is sent whenever an order on the book has been cancel-replaced. All remaining shares from the original order are no longer accessible, and must be removed. The new order details are provided for the replacement, along with a new order reference number which will be used henceforth. Since the side, stock symbol and attribution (if any) cannot be changed by an Order Replace event, these fields are not included in the message. Firms should retain the side, stock symbol and MPID from the original Add Order message.
#[itch_message(tag = b'U')]
pub struct OrderReplaceMessage {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// The original order reference number of the order being replaced
    #[field(offset = 11, len = 8)]
    original_order_reference_number: u64,
    /// The new reference number for this order at time of replacement
    /// Please note that the Nasdaq system will use this new order reference number for all subsequent updates
    #[field(offset = 19, len = 8)]
    new_order_reference_number: u64,
    /// The new total displayed quantity
    #[field(offset = 27, len = 4)]
    shares: u32,
    /// The new display price for the order
    /// Please refer to Data Types for field processing notes
    #[field(offset = 31, len = 4)]
    price: Price4,
}

/// Trade Message
/// The Trade Message is designed to provide execution details for normal match events involving non-displayable order types.
/// Since no Add Order Message is generated when a non-displayed order is initially received, Nasdaq cannot use the Order Executed messages for all matches. Therefore this message indicates when a match occurs between non-displayable order types. A Trade Message is transmitted each time a non-displayable order is executed in whole or in part. It is possible to receive multiple Trade Messages for the same order if that order is executed in several parts. Trade Messages for the same order are cumulative.
/// Trade Messages should be included in Nasdaq time-and-sales displays as well as volume and other market statistics. Since Trade Messages do not affect the book, however, they may be ignored by firms just looking to build and track the Nasdaq execution system display.
#[itch_message(tag = b'P')]
pub struct TradeMessage {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight.
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// The unique reference number assigned to the order on the book being executed.
    /// Effective December 6, 2010, Nasdaq will populate the Order Reference Number field within the Trade (Non- Cross) message as zero. For the binary versions of the TotalView-ITCH data feeds, the field will be null-filled bytes (which encodes sequence of zero)
    #[field(offset = 11, len = 8)]
    order_reference_number: u64,
    /// The type of non-display order on the book being matched
    /// Effective 07/14/2014, this field will always be “B” regardless of the resting side
    #[field(offset = 19, len = 1)]
    buy_sell_indicator: BuySellIndicator,
    /// The number of shares being matched in this execution
    #[field(offset = 20, len = 4)]
    shares: u32,
    /// Stock Symbol, right padded with spaces
    #[field(offset = 24, len = 8)]
    stock: &[u8],
    /// The match price of the order
    /// Please refer to Data Types for field processing notes
    #[field(offset = 32, len = 4)]
    price: Price4,
    /// The Nasdaq generated session unique Match Number for this trade
    /// The Match Number is referenced in the Trade Break Message
    #[field(offset = 36, len = 8)]
    match_number: u64,
}

/// Cross Trade Message
/// Cross Trade message indicates that Nasdaq has completed its cross process for a specific security. Nasdaq sends out
/// a Cross Trade message for all active issues in the system following the Opening, Closing and EMC cross events. Firms
/// may use the Cross Trade message to determine when the cross for each security has been completed.
///
/// For most issues, the Cross Trade message will indicate the bulk volume associated with the cross event. If the order
/// interest is insufficient to conduct a cross in a particular issue, however, the Cross Trade message may show the
/// shares as zero.
#[itch_message(tag = b'Q')]
pub struct CrossTradeMessage {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight.
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// The number of shares matched in the Nasdaq Cross.
    #[field(offset = 11, len = 8)]
    shares: u64,
    /// Stock symbol, right padded with spaces
    #[field(offset = 19, len = 8)]
    stock: &[u8],
    /// The price at which the cross occurred. Refer to Data Types for field processing notes.
    #[field(offset = 27, len = 4)]
    cross_price: Price4,
    /// The Nasdaq generated day-unique Match Number of this execution.
    #[field(offset = 31, len = 8)]
    match_number: u64,
    /// The Nasdaq cross session for which the message is being generated.
    #[field(offset = 39, len = 1)]
    cross_type: CrossType,
}

/// Broken Trade Message
/// The Broken Trade Message is sent whenever an execution on Nasdaq is broken. An execution may be broken if it is
/// found to be “clearly erroneous” pursuant to Nasdaq’s Clearly Erroneous Policy. A trade break is final; once a trade is
/// broken, it cannot be reinstated.
/// Firms that use the ITCH feed to create time-and-sales displays or calculate market statistics should be prepared
/// to process the broken trade message. If a firm is only using the ITCH feed to build a book, however, it may ignore
/// these messages as they have no impact on the current book.
#[itch_message(tag = b'B')]
pub struct BrokenTradeMessage {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight.
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// The Nasdaq Match Number of the execution that was broken. This refers to a Match Number from a previously
    /// transmitted Order Executed Message, Order Executed With Price Message, or Trade Message.
    #[field(offset = 11, len = 8)]
    match_number: u64,
}

/// Net Order Imbalance Indicator (NOII) Message
/// Nasdaq begins disseminating Net Order Imbalance Indicators (NOII) at 9:25 a.m. for the Opening Cross and 3:50 p.m. for the Closing Cross.
/// Between 9:25 and 9:28 a.m. and 3:50 and 3:55 p.m., Nasdaq disseminates the NOII information every 10 seconds.
/// Between 9:28 and 9:30 a.m. and 3:55 and 4:00 p.m., Nasdaq disseminates the NOII information every second.
/// For Nasdaq Halt, IPO and Pauses, NOII messages will be disseminated at 1 second intervals starting 1 second after quoting period starts/trading action is released.
/// Nasdaq will also disseminate an Extended Trading Close (ETC) message from 4:00 p.m. to 4:05 p.m. at five second intervals.
#[itch_message(tag = b'I')]
pub struct NetOrderImbalanceIndicatorMessage {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight.
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// The total number of shares that are eligible to be matched at the Current Reference Price.
    #[field(offset = 11, len = 8)]
    paired_shares: u64,
    /// The number of shares not paired at the Current Reference Price.
    #[field(offset = 19, len = 8)]
    imbalance_shares: u64,
    /// The market side of the order imbalance.
    #[field(offset = 27, len = 1)]
    imbalance_direction: ImbalanceDirection,
    /// Stock symbol, right padded with spaces
    #[field(offset = 28, len = 8)]
    stock: &[u8],
    /// A hypothetical auction-clearing price for cross orders only. Refer to Data Types for field processing notes.
    #[field(offset = 36, len = 4)]
    far_price: Price4,
    /// A hypothetical auction-clearing price for cross orders as well as continuous orders. Refer to Data Types for field processing notes.
    #[field(offset = 40, len = 4)]
    near_price: Price4,
    /// The price at which the NOII shares are being calculated. Refer to Data Types for field processing notes.
    #[field(offset = 44, len = 4)]
    current_reference_price: Price4,
    /// The type of Nasdaq cross for which the NOII message is being generated
    #[field(offset = 48, len = 1)]
    cross_type: CrossType,
    /// This field indicates the absolute value of the percentage of deviation of the Near Indicative Clearing Price to the nearest Current Reference Price.
    #[field(offset = 49, len = 1)]
    price_variation_indicator: PriceVariationIndicator,
}

#[repr(u8)]
pub enum ImbalanceDirection {
    /// buy imbalance
    Buy = b'B',
    /// sell imbalance
    Sell = b'S',
    /// no imbalance
    NoImbalance = b'N',
    /// Insufficient orders to calculate
    InsufficientOrders = b'O',
    /// Paused
    Paused = b'P',
    Unknown(u8),
}

impl From<u8> for ImbalanceDirection {
    fn from(value: u8) -> Self {
        match value {
            b'B' => Self::Buy,
            b'S' => Self::Sell,
            b'N' => Self::NoImbalance,
            b'O' => Self::InsufficientOrders,
            b'P' => Self::Paused,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum CrossType {
    /// Nasdaq Opening Cross
    NasdaqOpeningCross = b'O',
    /// Nasdaq Closing Cross
    NasdaqClosingCross = b'C',
    /// Cross for IPO and halted / paused securities
    IpoHaltedPaused = b'H',
    /// Extended Trading Close
    ExtendedTradingClose = b'A',
    Unknown(u8),
}

impl From<u8> for CrossType {
    fn from(value: u8) -> Self {
        match value {
            b'O' => Self::NasdaqOpeningCross,
            b'C' => Self::NasdaqClosingCross,
            b'H' => Self::IpoHaltedPaused,
            b'A' => Self::ExtendedTradingClose,
            unknown => Self::Unknown(unknown),
        }
    }
}

#[repr(u8)]
pub enum PriceVariationIndicator {
    /// Less than 1%
    LessThan1Percent = b'L',
    /// 1 to 1.99%
    Between1And1_99Percent = b'1',
    /// 2 to 2.99%
    Between2And2_99Percent = b'2',
    /// 3 to 3.99%
    Between3And3_99Percent = b'3',
    /// 4 to 4.99%
    Between4And4_99Percent = b'4',
    /// 5 to 5.99%
    Between5And5_99Percent = b'5',
    /// 6 to 6.99%
    Between6And6_99Percent = b'6',
    /// 7 to 7.99%
    Between7And7_99Percent = b'7',
    /// 8 to 8.99%
    Between8And8_99Percent = b'8',
    /// 9 to 9.99%
    Between9And9_99Percent = b'9',
    /// 10 to 19.99%
    Between10And19_99Percent = b'A',
    /// 20 to 29.99%
    Between20And29_99Percent = b'B',
    /// 30% or greater
    GreaterOrEqualTo30Percent = b'C',
    /// Cannot be calculated
    CannotBeCalculated = b' ',
    Unknown(u8),
}

impl From<u8> for PriceVariationIndicator {
    fn from(value: u8) -> Self {
        match value {
            b'L' => Self::LessThan1Percent,
            b'1' => Self::Between1And1_99Percent,
            b'2' => Self::Between2And2_99Percent,
            b'3' => Self::Between3And3_99Percent,
            b'4' => Self::Between4And4_99Percent,
            b'5' => Self::Between5And5_99Percent,
            b'6' => Self::Between6And6_99Percent,
            b'7' => Self::Between7And7_99Percent,
            b'8' => Self::Between8And8_99Percent,
            b'9' => Self::Between9And9_99Percent,
            b'A' => Self::Between10And19_99Percent,
            b'B' => Self::Between20And29_99Percent,
            b'C' => Self::GreaterOrEqualTo30Percent,
            b' ' => Self::CannotBeCalculated,
            unknown => Self::Unknown(unknown),
        }
    }
}

/// Retail Price Improvement Indicator (RPII)
/// Identifies a retail interest indication of the Bid, Ask or both the Bid and Ask for Nasdaq-listed securities.
#[itch_message(tag = b'N')]
pub struct RetailPriceImprovementIndicator {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight.
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// Stock symbol, right padded with spaces
    #[field(offset = 11, len = 8)]
    stock: &[u8],
    /// Interest Flag
    #[field(offset = 19, len = 1)]
    interest_flag: InterestFlag,
}

#[repr(u8)]
pub enum InterestFlag {
    /// RPI orders available on the buy side
    BuySide = b'B',
    /// RPI orders available on the sell side
    SellSide = b'S',
    /// RPI orders available on both sides (buy and sell)
    BothSides = b'A',
    /// No RPI orders available
    NoneAvailable = b'N',
    Unknown(u8),
}

impl From<u8> for InterestFlag {
    fn from(value: u8) -> Self {
        match value {
            b'B' => Self::BuySide,
            b'S' => Self::SellSide,
            b'A' => Self::BothSides,
            b'N' => Self::NoneAvailable,
            unknown => Self::Unknown(unknown),
        }
    }
}

/// Direct Listing with Capital Raise Price Discovery Message
/// The following message is disseminated only for Direct Listing with Capital Raise (DLCR) securities. Nasdaq begins
/// disseminating messages once per second as soon as the DLCR volatility test has successfully passed.
#[itch_message(tag = b'O')]
pub struct DirectListingwithCapitalRaisePriceDiscoveryMessage {
    /// Locate code identifying the security
    #[field(offset = 1, len = 2)]
    stock_locate: u16,
    /// Nasdaq internal tracking number
    #[field(offset = 3, len = 2)]
    tracking_number: u16,
    /// Nanoseconds since midnight
    #[field(offset = 5, len = 6)]
    timestamp: u64,
    /// Stock symbol, right padded with spaces
    #[field(offset = 11, len = 8)]
    stock: &[u8],
    /// Indicates if the security is eligible to be released for trading
    #[field(offset = 19, len = 1)]
    open_eligibility_status: OpenEligibilityStatus,
    /// 20% below Registration Statement Lower Price
    #[field(offset = 20, len = 4)]
    minimum_allowable_price: Price4,
    /// 80% above Registration Statement Highest Price
    #[field(offset = 24, len = 4)]
    maximum_allowable_price: Price4,
    /// The current reference price when the DLCR volatility test has successfully passed
    #[field(offset = 28, len = 4)]
    near_execution_price: Price4,
    /// The time at which the near execution price was set
    #[field(offset = 32, len = 8)]
    near_execution_time: u64,
    /// Indicates the price of the Lower Auction Collar Threshold (10% below the Near Execution Price)
    #[field(offset = 40, len = 4)]
    lower_price_range_collar: Price4,
    /// Indicates the price of the Upper Auction Collar Threshold (10% above the Near Execution Price)
    #[field(offset = 44, len = 4)]
    upper_price_range_collar: Price4,
}

#[repr(u8)]
pub enum OpenEligibilityStatus {
    /// Not Eligible
    NotEligible = b'N',
    /// Eligible
    Eligible = b'Y',
    Unknown(u8),
}

impl From<u8> for OpenEligibilityStatus {
    fn from(value: u8) -> Self {
        match value {
            b'N' => Self::NotEligible,
            b'Y' => Self::Eligible,
            unknown => Self::Unknown(unknown),
        }
    }
}

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

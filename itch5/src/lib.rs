use itch5_derive::itch_message;

pub enum MessageKind {
    /// System Event Message
    /// The system event message type is used to signal a market or data feed handler event.
    SystemEventMessage,

    /// Stock Directory
    /// At the start of each trading day, Nasdaq disseminates stock directory messages for all active symbols in the Nasdaq
    /// execution system.
    /// Market data redistributors should process this message to populate the Financial Status Indicator (required display
    /// field) and the Market Category (recommended display field) for Nasdaq listed issues.
    StockDirectory,

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
    StockTradingAction,

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
    RegSHORestriction,

    /// Market Participant Position
    /// At the start of each trading day, Nasdaq disseminates a spin of market participant position messages. The
    /// message provides the Primary Market Maker status, Market Maker mode and Market Participant state for
    /// each Nasdaq market participant firm registered in an issue. Market participant firms may use these fields to
    /// comply with certain marketplace rules.
    /// Throughout the day, Nasdaq will send out this message only if Nasdaq Operations changes the status of a
    /// market participant firm in an issue.
    MarketParticipantPosition,

    /// Market-Wide Circuit Breaker (MWCB) Decline Level Message
    /// Informs data recipients what the daily MWCB breach points are set to for the current trading day.
    MWCBDeclineLevelMessage,

    /// Market-Wide Circuit Breaker (MWCB) Status Message
    /// Informs data recipients when a MWCB has breached one of the established levels
    MWCBStatusMessage,

    /// Indicates the anticipated IPO quotation release time of a security.
    QuotingPeriodUpdate,

    /// Limit Up – Limit Down (LULD) Auction Collar
    /// Indicates the auction collar thresholds within which a paused security can reopen following a LULD Trading Pause.
    LULDAuctionCollar,

    /// The Exchange uses this message to indicate the current Operational Status of a security to the trading
    /// community. An Operational Halt means that there has been an interruption of service on the identified
    /// security impacting only the designated Market Center. These Halts differ from the “Stock Trading
    /// Action” message types since an Operational Halt is specific to the exchange for which it is declared, and
    /// does not interrupt the ability of the trading community to trade the identified instrument on any other
    /// marketplace.
    /// Nasdaq uses this administrative message to indicate the current trading status of the three market centers
    /// operated by Nasdaq.
    OperationalHalt,

    /// Add Order - No MPID Attribution
    /// This message will be generated for unattributed orders accepted by the Nasdaq system. (Note: If a firm wants to
    /// display a MPID for unattributed orders, Nasdaq recommends that it use the MPID of “NSDQ”.)
    AddOrderNoMPIDAttribution,

    /// This message will be generated for attributed orders and quotations accepted by the Nasdaq system.
    AddOrderWithMPIDAttribution,

    /// Order Executed Message
    /// This message is sent whenever an order on the book is executed in whole or in part. It is possible to receive several
    /// Order Executed Messages for the same order reference number if that order is executed in several parts. The
    /// multiple Order Executed Messages on the same order are cumulative.
    /// By combining the executions from both types of Order Executed Messages and the Trade Message, it is possible to
    /// build a complete view of all non-•-cross executions that happen on Nasdaq. Cross execution information is available in
    /// one bulk print per symbol via the Cross Trade Message.
    OrderExecutedMessage,

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
    OrderExecutedWithPriceMessage,

    /// Order Cancel Message
    /// This message is sent whenever an order on the book is modified as a result of a partial cancellation.
    OrderCancelMessage,

    /// Order Delete Message
    /// This message is sent whenever an order on the book is being cancelled. All remaining shares are no longer
    /// accessible so the order must be removed from the book.
    OrderDeleteMessage,

    /// Order Replace Message
    /// This message is sent whenever an order on the book has been cancel-•-replaced. All remaining shares from the
    /// original order are no longer accessible, and must be removed. The new order details are provided for the
    /// replacement, along with a new order reference number which will be used henceforth. Since the side, stock
    /// symbol and attribution (if any) cannot be changed by an Order Replace event, these fields are not included in the
    /// message. Firms should retain the side, stock symbol and MPID from the original Add Order message.
    OrderReplaceMessage,

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
    TradeMessage,

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
    CrossTradeMessage,

    /// Broken Trade / Order Execution Message
    /// The Broken Trade Message is sent whenever an execution on Nasdaq is broken. An execution may be broken if it is
    /// found to be “clearly erroneous” pursuant to Nasdaq’s Clearly Erroneous Policy. A trade break is final; once a trade is
    /// broken, it cannot be reinstated.
    /// Firms that use the ITCH feed to create time---and---sales displays or calculate market statistics should be prepared
    /// to process the broken trade message. If a firm is only using the ITCH feed to build a book, however, it may ignore
    /// these messages as they have no impact on the current book.
    BrokenTradeMessage,

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
    NetOrderImbalanceIndicatorMessage,

    /// Retail Price Improvement Indicator (RPII)
    /// Identifies a retail interest indication of the Bid, Ask or both the Bid and Ask for Nasdaq-•-listed securities.
    RetailPriceImprovementIndicator,

    /// Direct Listing with Capital Raise Price Discovery Message
    /// The following message is disseminated only for Direct Listing with Capital Raise (DLCR) securities. Nasdaq begins
    /// disseminating messages once per second as soon as the DLCR volatility test has successfully passed.
    DirectListingwithCapitalRaisePriceDiscoveryMessage,
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

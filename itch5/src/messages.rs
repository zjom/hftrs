use std::str::FromStr;

use zerocopy::{
    FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned,
    network_endian::{U16, U32, U64},
};

/// Prices are integer fields, supplied with an associated precision. When converted to a decimal format, prices are in
/// fixed point format, where the precision defines the number of decimal places. For example, a field flagged as Price
/// (4) has an implied 4 decimal places. The maximum value of price (4) in TotalView ITCH is 200,000.0000 (decimal,
/// 77359400 hex).
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Copy, Clone)]
#[repr(transparent)]
pub struct Price4([u8; 4]);

impl Price4 {
    pub fn into_u32(&self) -> u32 {
        u32::from_be_bytes(self.0)
    }

    pub fn into_f64(&self) -> f64 {
        f64::from(self.into_u32()) / 10000.0
    }

    pub fn into_i64(&self) -> i64 {
        self.into_u32() as i64
    }
}

/// Prices are integer fields, supplied with an associated precision. When converted to a decimal format, prices are in
/// fixed point format, where the precision defines the number of decimal places.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Copy, Clone)]
#[repr(transparent)]
pub struct Price8([u8; 8]);

impl Price8 {
    pub fn into_u64(&self) -> u64 {
        u64::from_be_bytes(self.0)
    }

    pub fn into_f64(&self) -> f64 {
        self.into_u64() as f64 / 1_0000_0000.0
    }
}

#[derive(
    FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Copy, Clone, PartialEq,
)]
#[repr(transparent)]
pub struct Symbol([u8; 8]);
impl Symbol {
    #[inline]
    pub const fn hash(&self) -> u64 {
        u64::from_be_bytes(self.0)
    }

    #[inline]
    pub const fn from_hash(hash: u64) -> Symbol {
        Symbol(hash.to_be_bytes())
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        str::from_utf8(&self.0).unwrap_or("unknown")
    }
}

impl FromStr for Symbol {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() == 0 {
            return Err("empty input");
        }

        let mut arr = [b' '; 8];
        let upper = s.to_uppercase();
        let bytes = upper.as_bytes();
        let len = bytes.len().min(8);
        arr[..len].copy_from_slice(&bytes[..len]);
        Ok(Self(arr))
    }
}

/// System Event Message
/// The system event message type is used to signal a market or data feed handler event. The format is as follows:
/// Name Offset Length Value Notes
/// Message Type 0 1 "S" System Event Message
/// Stock Locate 1 2 Integer Always 0
/// Tracking Number 3 2 Integer Nasdaq internal tracking number
/// Timestamp 5 6 Integer Nanoseconds since midnight
/// Event Code 11 1 Alpha See System Event Codes below
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct SystemEvent {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    event_code: u8,
}

impl SystemEvent {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'S';

    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    pub fn event_code(&self) -> SystemEventCode {
        SystemEventCode::from(self.event_code)
    }
}

/// Nasdaq supports the following event codes on a daily basis on the TotalView-ITCH data feed.
/// Code Explanation
/// "O" Start of Messages. Outside of time stamp messages, the start of day message is the first message sent in
/// any trading day.
/// "S" Start of System hours. This message indicates that NASDAQ is open and ready to start accepting orders.
/// "Q" Start of Market hours. This message is intended to indicate that Market Hours orders are available
/// for execution.
/// "M" End of Market hours. This message is intended to indicate that Market Hours orders are no longer
/// available for execution.
/// "E" End of System hours. It indicates that Nasdaq is now closed and will not accept any new orders today.
/// It is still possible to receive Broken Trade messages and Order Delete messages after the End of Day
/// ."C" End of Messages. This is always the last message sent in any trading day.
#[derive(Debug)]
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct StockDirectory {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    stock: Symbol,
    market_category: u8,
    financial_status_indicator: u8,
    round_lot_size: U32,
    round_lots_only: u8,
    issue_classification: u8,
    issue_sub_type: [u8; 2],
    authenticity: u8,
    short_sale_threshold_indicator: u8,
    ipo_flag: u8,
    luld_reference_price_tier: u8,
    etp_flag: u8,
    etp_leverage_factor: U32,
    inverse_indicator: u8,
}

impl StockDirectory {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'R';

    /// Locate Code uniquely assigned to the security symbol for the day.
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Time at which the directory message was generated. Refer to Data Types for field processing notes.
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// Denotes the security symbol for the issue in the Nasdaq execution system.
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// Indicates Listing market or listing market tier for the issue
    pub fn market_category(&self) -> MarketCategory {
        MarketCategory::from(self.market_category)
    }
    /// For Nasdaq listed issues, this field indicates when a firm is not in compliance with Nasdaq continued listing requirements
    pub fn financial_status_indicator(&self) -> FinancialStatusIndicator {
        FinancialStatusIndicator::from(self.financial_status_indicator)
    }
    /// Denotes the number of shares that represent a round lot for the issue
    pub fn round_lot_size(&self) -> u32 {
        self.round_lot_size.get()
    }
    /// Indicates if Nasdaq system limits order entry for issue
    pub fn round_lots_only(&self) -> RoundLotsOnly {
        RoundLotsOnly::from(self.round_lots_only)
    }
    /// Identifies the security class for the issue as assigned by Nasdaq. See Appendix for allowable values.
    pub fn issue_classification(&self) -> u8 {
        self.issue_classification
    }
    /// Identifies the security sub-type for the issue as assigned by Nasdaq. See Appendix for allowable values.
    pub fn issue_sub_type(&self) -> &[u8] {
        &self.issue_sub_type
    }
    /// Denotes if an issue or quoting participant record is set-up in Nasdaq systems in a live/production, test, or demo state.
    pub fn authenticity(&self) -> Authenticity {
        Authenticity::from(self.authenticity)
    }
    /// Indicates if a security is subject to mandatory close-out of short sales under SEC Rule 203(b)(3).
    pub fn short_sale_threshold_indicator(&self) -> ShortSaleThresholdIndicator {
        ShortSaleThresholdIndicator::from(self.short_sale_threshold_indicator)
    }
    /// Indicates if the Nasdaq security is set up for IPO release.
    pub fn ipo_flag(&self) -> IpoFlag {
        IpoFlag::from(self.ipo_flag)
    }
    /// Indicates which Limit Up / Limit Down price band calculation parameter is to be used for the instrument.
    pub fn luld_reference_price_tier(&self) -> LuldReferencePriceTier {
        LuldReferencePriceTier::from(self.luld_reference_price_tier)
    }
    /// Indicates whether the security is an exchange traded product (ETP).
    pub fn etp_flag(&self) -> EtpFlag {
        EtpFlag::from(self.etp_flag)
    }
    /// Tracks the integral relationship of the ETP to the underlying index.
    pub fn etp_leverage_factor(&self) -> u32 {
        self.etp_leverage_factor.get()
    }
    /// Indicates the directional relationship between the ETP and Underlying index.
    pub fn inverse_indicator(&self) -> InverseIndicator {
        InverseIndicator::from(self.inverse_indicator)
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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
/// Stock Trading Action message with the "T" (Trading Resumption) for all Nasdaq--- and other exchange-•-listed
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct StockTradingAction {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    stock: Symbol,
    trading_state: u8,
    reserved: u8,
    reason: [u8; 4],
}

impl StockTradingAction {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'H';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// Stock symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// Indicates the current trading state for the stock.
    pub fn trading_state(&self) -> TradingState {
        TradingState::from(self.trading_state)
    }
    /// Reserved.
    pub fn reserved(&self) -> u8 {
        self.reserved
    }
    /// Trading Action reason.
    pub fn reason(&self) -> &[u8] {
        &self.reason
    }
}

#[derive(Debug)]
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct RegSHORestriction {
    tag: u8,
    locate_code: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    stock: Symbol,
    reg_sho_action: u8,
}

impl RegSHORestriction {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'Y';

    /// Locate code identifying the security
    pub fn locate_code(&self) -> u16 {
        self.locate_code.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// Stock symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// Denotes the Reg SHO Short Sale Price Test Restriction status for the issue at the time of the message dissemination.
    pub fn reg_sho_action(&self) -> RegShoAction {
        RegShoAction::from(self.reg_sho_action)
    }
}

#[derive(Debug)]
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct MarketParticipantPosition {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    mpid: [u8; 4],
    stock: Symbol,
    primary_market_maker: u8,
    market_maker_mode: u8,
    market_participant_state: u8,
}

impl MarketParticipantPosition {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'L';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// Denotes the market participant identifier for which the position message is being generated
    pub fn mpid(&self) -> &[u8] {
        &self.mpid
    }
    /// Stock symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// Indicates if the market participant firm qualifies as a Primary Market Maker in accordance with Nasdaq marketplace rules
    pub fn primary_market_maker(&self) -> PrimaryMarketMaker {
        PrimaryMarketMaker::from(self.primary_market_maker)
    }
    /// Indicates the quoting participant's registration status in relation to SEC Rules 101 and 104 of Regulation M
    pub fn market_maker_mode(&self) -> MarketMakerMode {
        MarketMakerMode::from(self.market_maker_mode)
    }
    /// Indicates the market participant's current registration status in the issue
    pub fn market_participant_state(&self) -> MarketParticipantState {
        MarketParticipantState::from(self.market_participant_state)
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct MWCBDeclineLevel {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    level_1: Price8,
    level_2: Price8,
    level_3: Price8,
}

impl MWCBDeclineLevel {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'V';

    /// Always set to 0
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Time at which the MWCB Decline Level message was generated
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// Denotes the MWCB Level 1 Value.
    pub fn level_1(&self) -> &Price8 {
        &self.level_1
    }
    /// Denotes the MWCB Level 2 Value.
    pub fn level_2(&self) -> &Price8 {
        &self.level_2
    }
    /// Denotes the MWCB Level 3 Value.
    pub fn level_3(&self) -> &Price8 {
        &self.level_3
    }
}

/// Market-Wide Circuit Breaker Status message
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct MWCBStatus {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    breached_level: u8,
}

impl MWCBStatus {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'W';

    /// Always set to 0
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Time at which the MWCB Breaker Status message was generated
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// Denotes the MWCB Level that was breached.
    pub fn breached_level(&self) -> BreachedLevel {
        BreachedLevel::from(self.breached_level)
    }
}

#[derive(Debug)]
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct QuotingPeriodUpdate {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    stock: Symbol,
    ipo_quotation_release_time: U32,
    ipo_quotation_release_qualifier: u8,
    ipo_price: Price4,
}

impl QuotingPeriodUpdate {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'K';

    /// Always set to 0
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Time at which the IPO Quoting Period Update message was generated
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// Stock symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// Denotes the IPO release time, in seconds since midnight, for quotation to the nearest second.
    pub fn ipo_quotation_release_time(&self) -> u32 {
        self.ipo_quotation_release_time.get()
    }
    /// Anticipated Quotation Release Time or IPO Release Canceled/Postponed
    pub fn ipo_quotation_release_qualifier(&self) -> IpoQuotationReleaseQualifier {
        IpoQuotationReleaseQualifier::from(self.ipo_quotation_release_qualifier)
    }
    /// Denotes the IPO Price to be used for intraday net change calculations.
    pub fn ipo_price(&self) -> &Price4 {
        &self.ipo_price
    }
}

#[derive(Debug)]
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct LULDAuctionCollar {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    stock: Symbol,
    auction_collar_reference_price: Price4,
    upper_auction_collar_price: Price4,
    lower_auction_collar_price: Price4,
    auction_collar_extension: Price4,
}

impl LULDAuctionCollar {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'J';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds past midnight
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// Stock symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// Reference price used to set the Auction Collars
    pub fn auction_collar_reference_price(&self) -> &Price4 {
        &self.auction_collar_reference_price
    }
    /// Indicates the price of the Upper Auction Collar Threshold
    pub fn upper_auction_collar_price(&self) -> &Price4 {
        &self.upper_auction_collar_price
    }
    /// Indicates the price of the Lower Auction Collar Threshold
    pub fn lower_auction_collar_price(&self) -> &Price4 {
        &self.lower_auction_collar_price
    }
    /// Indicates the number of the extensions to the Reopening Auction
    pub fn auction_collar_extension(&self) -> &Price4 {
        &self.auction_collar_extension
    }
}

/// Operational Halt Message
/// The Exchange uses this message to indicate the current Operational Status of a security to the trading
/// community. An Operational Halt means that there has been an interruption of service on the identified
/// security impacting only the designated Market Center. These Halts differ from the "Stock Trading
/// Action" message types since an Operational Halt is specific to the exchange for which it is declared, and
/// does not interrupt the ability of the trading community to trade the identified instrument on any other
/// marketplace.
/// Nasdaq uses this administrative message to indicate the current trading status of the three market centers
/// operated by Nasdaq.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct OperationalHalt {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    stock: Symbol,
    market_code: u8,
    operational_halt_action: u8,
}

impl OperationalHalt {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'h';

    /// Locate code uniquely assigned to the security symbol for the day.
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Time at which the Operational Halt message was generated.
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// Denotes the security symbol for the issue in Nasdaq execution system
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// Market Code
    pub fn market_code(&self) -> MarketCode {
        MarketCode::from(self.market_code)
    }
    /// Operational Halt Action
    pub fn operational_halt_action(&self) -> OperationalHaltAction {
        OperationalHaltAction::from(self.operational_halt_action)
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
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
/// This message will be generated for unattributed orders accepted by the Nasdaq system. (Note: If a firm wants to display a MPID for unattributed orders, Nasdaq recommends that it use the MPID of "NSDQ".)
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct AddOrderNoMPIDAttribution {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    order_reference_number: U64,
    buy_sell_indicator: u8,
    shares: U32,
    stock: Symbol,
    price: Price4,
}

impl AddOrderNoMPIDAttribution {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'A';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight.
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// The unique reference number assigned to the new order at the time of receipt.
    pub fn order_reference_number(&self) -> u64 {
        self.order_reference_number.get()
    }
    /// The type of order being added.
    pub fn buy_sell_indicator(&self) -> BuySellIndicator {
        BuySellIndicator::from(self.buy_sell_indicator)
    }
    /// The total number of shares associated with the order being added to the book.
    pub fn shares(&self) -> u32 {
        self.shares.get()
    }
    /// Stock symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// The display price of the new order. Refer to Data Types for field processing notes.
    pub fn price(&self) -> &Price4 {
        &self.price
    }
}

#[derive(Debug)]
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct AddOrderWithMPIDAttribution {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    order_reference_number: U64,
    buy_sell_indicator: u8,
    shares: U32,
    stock: Symbol,
    price: Price4,
    attribution: [u8; 4],
}

impl AddOrderWithMPIDAttribution {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'F';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight.
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// The unique reference number assigned to the new order at the time of receipt.
    pub fn order_reference_number(&self) -> u64 {
        self.order_reference_number.get()
    }
    /// The type of order being added.
    pub fn buy_sell_indicator(&self) -> BuySellIndicator {
        BuySellIndicator::from(self.buy_sell_indicator)
    }
    /// The total number of shares associated with the order being added to the book
    pub fn shares(&self) -> u32 {
        self.shares.get()
    }
    /// Stock symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// The display price of the new order. Refer to Data Types for field processing notes.
    pub fn price(&self) -> &Price4 {
        &self.price
    }
    /// Nasdaq Market participant identifier associated with the entered order
    pub fn attribution(&self) -> &[u8] {
        &self.attribution
    }
}

/// Order Executed Message
/// This message is sent whenever an order on the book is executed in whole or in part. It is possible to receive several
/// Order Executed Messages for the same order reference number if that order is executed in several parts. The
/// multiple Order Executed Messages on the same order are cumulative.
/// By combining the executions from both types of Order Executed Messages and the Trade Message, it is possible to
/// build a complete view of all non-•-cross executions that happen on Nasdaq. Cross execution information is available in one bulk print per symbol via the Cross Trade Message.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct OrderExecuted {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    order_reference_number: U64,
    executed_shares: U32,
    match_number: U64,
}

impl OrderExecuted {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'E';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// The unique reference number assigned to the new order at the time of receipt
    pub fn order_reference_number(&self) -> u64 {
        self.order_reference_number.get()
    }
    /// The number of shares executed
    pub fn executed_shares(&self) -> u32 {
        self.executed_shares.get()
    }
    /// The Nasdaq generated day unique Match Number of this execution. The Match Number is also referenced in the Trade Break Message
    pub fn match_number(&self) -> u64 {
        self.match_number.get()
    }
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct OrderExecutedWithPrice {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    order_reference_number: U64,
    executed_shares: U32,
    match_number: U64,
    printable: u8,
    execution_price: Price4,
}

impl OrderExecutedWithPrice {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'C';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// The unique reference number assigned to the new order at the time of receipt
    pub fn order_reference_number(&self) -> u64 {
        self.order_reference_number.get()
    }
    /// The number of shares executed
    pub fn executed_shares(&self) -> u32 {
        self.executed_shares.get()
    }
    /// The Nasdaq generated day unique Match Number of this execution. The Match Number is also referenced in the Trade Break Message
    pub fn match_number(&self) -> u64 {
        self.match_number.get()
    }
    /// Indicates if the execution should be reflected on time and sales displays and volume calculations
    pub fn printable(&self) -> Printable {
        Printable::from(self.printable)
    }
    /// The Price at which the order execution occurred. Refer to Data Types for field processing notes
    pub fn execution_price(&self) -> &Price4 {
        &self.execution_price
    }
}

#[derive(Debug)]
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct OrderCancel {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    order_reference_number: U64,
    cancelled_shares: U32,
}

impl OrderCancel {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'X';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// The reference number of the order being canceled
    pub fn order_reference_number(&self) -> u64 {
        self.order_reference_number.get()
    }
    /// The number of shares being removed from the display size of the order as a result of a cancellation
    pub fn cancelled_shares(&self) -> u32 {
        self.cancelled_shares.get()
    }
}

/// Order Delete Message
/// This message is sent whenever an order on the book is being cancelled. All remaining shares are no longer accessible so the order must be removed from the book.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct OrderDelete {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    order_reference_number: U64,
}

impl OrderDelete {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'D';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// The reference number of the order being canceled
    pub fn order_reference_number(&self) -> u64 {
        self.order_reference_number.get()
    }
}

/// Order Replace Message
/// This message is sent whenever an order on the book has been cancel-replaced. All remaining shares from the original order are no longer accessible, and must be removed. The new order details are provided for the replacement, along with a new order reference number which will be used henceforth. Since the side, stock symbol and attribution (if any) cannot be changed by an Order Replace event, these fields are not included in the message. Firms should retain the side, stock symbol and MPID from the original Add Order message.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct OrderReplace {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    original_order_reference_number: U64,
    new_order_reference_number: U64,
    shares: U32,
    price: Price4,
}

impl OrderReplace {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'U';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// The original order reference number of the order being replaced
    pub fn original_order_reference_number(&self) -> u64 {
        self.original_order_reference_number.get()
    }
    /// The new reference number for this order at time of replacement
    pub fn new_order_reference_number(&self) -> u64 {
        self.new_order_reference_number.get()
    }
    /// The new total displayed quantity
    pub fn shares(&self) -> u32 {
        self.shares.get()
    }
    /// The new display price for the order
    pub fn price(&self) -> &Price4 {
        &self.price
    }
}

/// Trade Message
/// The Trade Message is designed to provide execution details for normal match events involving non-displayable order types.
/// Since no Add Order Message is generated when a non-displayed order is initially received, Nasdaq cannot use the Order Executed messages for all matches. Therefore this message indicates when a match occurs between non-displayable order types. A Trade Message is transmitted each time a non-displayable order is executed in whole or in part. It is possible to receive multiple Trade Messages for the same order if that order is executed in several parts. Trade Messages for the same order are cumulative.
/// Trade Messages should be included in Nasdaq time-and-sales displays as well as volume and other market statistics. Since Trade Messages do not affect the book, however, they may be ignored by firms just looking to build and track the Nasdaq execution system display.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct Trade {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    order_reference_number: U64,
    buy_sell_indicator: u8,
    shares: U32,
    stock: Symbol,
    price: Price4,
    match_number: U64,
}

impl Trade {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'P';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight.
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// The unique reference number assigned to the order on the book being executed.
    pub fn order_reference_number(&self) -> u64 {
        self.order_reference_number.get()
    }
    /// The type of non-display order on the book being matched
    pub fn buy_sell_indicator(&self) -> BuySellIndicator {
        BuySellIndicator::from(self.buy_sell_indicator)
    }
    /// The number of shares being matched in this execution
    pub fn shares(&self) -> u32 {
        self.shares.get()
    }
    /// Stock Symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// The match price of the order
    pub fn price(&self) -> &Price4 {
        &self.price
    }
    /// The Nasdaq generated session unique Match Number for this trade
    pub fn match_number(&self) -> u64 {
        self.match_number.get()
    }
}

/// Cross Trade Message
/// Cross Trade message indicates that Nasdaq has completed its cross process for a specific security. Nasdaq sends out
/// a Cross Trade message for all active issues in the system following the Opening, Closing and EMC cross events. Firms
/// may use the Cross Trade message to determine when the cross for each security has been completed.
///
/// For most issues, the Cross Trade message will indicate the bulk volume associated with the cross event. If the order
/// interest is insufficient to conduct a cross in a particular issue, however, the Cross Trade message may show the
/// shares as zero.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct CrossTrade {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    shares: U64,
    stock: Symbol,
    cross_price: Price4,
    match_number: U64,
    cross_type: u8,
}

impl CrossTrade {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'Q';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight.
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// The number of shares matched in the Nasdaq Cross.
    pub fn shares(&self) -> u64 {
        self.shares.get()
    }
    /// Stock symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// The price at which the cross occurred. Refer to Data Types for field processing notes.
    pub fn cross_price(&self) -> &Price4 {
        &self.cross_price
    }
    /// The Nasdaq generated day-unique Match Number of this execution.
    pub fn match_number(&self) -> u64 {
        self.match_number.get()
    }
    /// The Nasdaq cross session for which the message is being generated.
    pub fn cross_type(&self) -> CrossType {
        CrossType::from(self.cross_type)
    }
}

/// Broken Trade Message
/// The Broken Trade Message is sent whenever an execution on Nasdaq is broken. An execution may be broken if it is
/// found to be "clearly erroneous" pursuant to Nasdaq's Clearly Erroneous Policy. A trade break is final; once a trade is
/// broken, it cannot be reinstated.
/// Firms that use the ITCH feed to create time-and-sales displays or calculate market statistics should be prepared
/// to process the broken trade message. If a firm is only using the ITCH feed to build a book, however, it may ignore
/// these messages as they have no impact on the current book.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct BrokenTrade {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    match_number: U64,
}

impl BrokenTrade {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'B';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight.
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// The Nasdaq Match Number of the execution that was broken.
    pub fn match_number(&self) -> u64 {
        self.match_number.get()
    }
}

/// Net Order Imbalance Indicator (NOII) Message
/// Nasdaq begins disseminating Net Order Imbalance Indicators (NOII) at 9:25 a.m. for the Opening Cross and 3:50 p.m. for the Closing Cross.
/// Between 9:25 and 9:28 a.m. and 3:50 and 3:55 p.m., Nasdaq disseminates the NOII information every 10 seconds.
/// Between 9:28 and 9:30 a.m. and 3:55 and 4:00 p.m., Nasdaq disseminates the NOII information every second.
/// For Nasdaq Halt, IPO and Pauses, NOII messages will be disseminated at 1 second intervals starting 1 second after quoting period starts/trading action is released.
/// Nasdaq will also disseminate an Extended Trading Close (ETC) message from 4:00 p.m. to 4:05 p.m. at five second intervals.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct NetOrderImbalanceIndicator {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    paired_shares: U64,
    imbalance_shares: U64,
    imbalance_direction: u8,
    stock: Symbol,
    far_price: Price4,
    near_price: Price4,
    current_reference_price: Price4,
    cross_type: u8,
    price_variation_indicator: u8,
}

impl NetOrderImbalanceIndicator {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'I';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight.
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// The total number of shares that are eligible to be matched at the Current Reference Price.
    pub fn paired_shares(&self) -> u64 {
        self.paired_shares.get()
    }
    /// The number of shares not paired at the Current Reference Price.
    pub fn imbalance_shares(&self) -> u64 {
        self.imbalance_shares.get()
    }
    /// The market side of the order imbalance.
    pub fn imbalance_direction(&self) -> ImbalanceDirection {
        ImbalanceDirection::from(self.imbalance_direction)
    }
    /// Stock symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// A hypothetical auction-clearing price for cross orders only.
    pub fn far_price(&self) -> &Price4 {
        &self.far_price
    }
    /// A hypothetical auction-clearing price for cross orders as well as continuous orders.
    pub fn near_price(&self) -> &Price4 {
        &self.near_price
    }
    /// The price at which the NOII shares are being calculated.
    pub fn current_reference_price(&self) -> &Price4 {
        &self.current_reference_price
    }
    /// The type of Nasdaq cross for which the NOII message is being generated
    pub fn cross_type(&self) -> CrossType {
        CrossType::from(self.cross_type)
    }
    /// This field indicates the absolute value of the percentage of deviation of the Near Indicative Clearing Price to the nearest Current Reference Price.
    pub fn price_variation_indicator(&self) -> PriceVariationIndicator {
        PriceVariationIndicator::from(self.price_variation_indicator)
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug)]
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct RetailPriceImprovementIndicator {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    stock: Symbol,
    interest_flag: u8,
}

impl RetailPriceImprovementIndicator {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'N';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight.
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// Stock symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// Interest Flag
    pub fn interest_flag(&self) -> InterestFlag {
        InterestFlag::from(self.interest_flag)
    }
}

#[derive(Debug)]
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
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug)]
#[repr(C, packed)]
pub struct DirectListingwithCapitalRaisePriceDiscovery {
    tag: u8,
    stock_locate: U16,
    tracking_number: U16,
    timestamp: [u8; 6],
    stock: Symbol,
    open_eligibility_status: u8,
    minimum_allowable_price: Price4,
    maximum_allowable_price: Price4,
    near_execution_price: Price4,
    near_execution_time: U64,
    lower_price_range_collar: Price4,
    upper_price_range_collar: Price4,
}

impl DirectListingwithCapitalRaisePriceDiscovery {
    pub const LEN: usize = size_of::<Self>();
    pub const TAG: u8 = b'O';

    /// Locate code identifying the security
    pub fn stock_locate(&self) -> u16 {
        self.stock_locate.get()
    }
    /// Nasdaq internal tracking number
    pub fn tracking_number(&self) -> u16 {
        self.tracking_number.get()
    }
    /// Nanoseconds since midnight
    pub fn timestamp(&self) -> u64 {
        read_u48(&self.timestamp)
    }
    /// Stock symbol, right padded with spaces
    pub fn stock(&self) -> &Symbol {
        &self.stock
    }
    /// Indicates if the security is eligible to be released for trading
    pub fn open_eligibility_status(&self) -> OpenEligibilityStatus {
        OpenEligibilityStatus::from(self.open_eligibility_status)
    }
    /// 20% below Registration Statement Lower Price
    pub fn minimum_allowable_price(&self) -> &Price4 {
        &self.minimum_allowable_price
    }
    /// 80% above Registration Statement Highest Price
    pub fn maximum_allowable_price(&self) -> &Price4 {
        &self.maximum_allowable_price
    }
    /// The current reference price when the DLCR volatility test has successfully passed
    pub fn near_execution_price(&self) -> &Price4 {
        &self.near_execution_price
    }
    /// The time at which the near execution price was set
    pub fn near_execution_time(&self) -> u64 {
        self.near_execution_time.get()
    }
    /// Indicates the price of the Lower Auction Collar Threshold (10% below the Near Execution Price)
    pub fn lower_price_range_collar(&self) -> &Price4 {
        &self.lower_price_range_collar
    }
    /// Indicates the price of the Upper Auction Collar Threshold (10% above the Near Execution Price)
    pub fn upper_price_range_collar(&self) -> &Price4 {
        &self.upper_price_range_collar
    }
}

#[derive(Debug)]
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

// -- private utils
#[inline]
fn read_u48(bytes: &[u8; 6]) -> u64 {
    let mut b = [0u8; 8];
    b[2..].copy_from_slice(bytes);
    u64::from_be_bytes(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_symbol_eq_unhash() {
        let input = "aapl";
        let s: Symbol = input.parse().expect("should not panic");

        assert_eq!(Symbol::from_hash(s.hash()), s)
    }

    /// ITCH5 wire format right-pads symbols with ASCII spaces. `from_str` must
    /// produce the same byte pattern so a user-supplied watch symbol hashes to
    /// the same value as the symbol carried in a StockDirectory message.
    #[test]
    fn from_str_pads_with_spaces() {
        let s: Symbol = "AAPL".parse().unwrap();
        assert_eq!(&s.0, b"AAPL    ");
    }

    #[test]
    fn from_str_hash_matches_wire_symbol() {
        let from_user: Symbol = "AAPL".parse().unwrap();
        let from_wire = Symbol(*b"AAPL    ");
        assert_eq!(from_user.hash(), from_wire.hash());
        assert_eq!(from_user, from_wire);
    }

    #[test]
    fn from_str_uppercases() {
        let lower: Symbol = "aapl".parse().unwrap();
        let upper: Symbol = "AAPL".parse().unwrap();
        assert_eq!(lower.hash(), upper.hash());
    }

    #[test]
    fn from_str_full_eight_chars_not_padded() {
        let s: Symbol = "ABCDEFGH".parse().unwrap();
        assert_eq!(&s.0, b"ABCDEFGH");
    }

    #[test]
    fn from_str_truncates_overlong_input() {
        let s: Symbol = "ABCDEFGHIJ".parse().unwrap();
        assert_eq!(&s.0, b"ABCDEFGH");
    }

    #[test]
    fn from_str_empty_is_err() {
        assert!("".parse::<Symbol>().is_err());
    }
}

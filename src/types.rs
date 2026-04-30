//! Core domain types.
//!
//! Types are deliberately POD and `Copy` where possible to keep the hot path
//! free of allocation and reference juggling. Newtype wrappers around
//! [`OrderId`] / [`Price`] / [`Quantity`] would buy us extra type safety; left
//! as a follow-up so the scaffold stays readable.

/// Exchange-assigned order identifier. ITCH uses `u64`.
pub type OrderId = u64;

/// Price expressed in ticks. ITCH 5.0 transmits prices as `u32` with four
/// implicit decimals (i.e. `$100.0000` is `1_000_000`). We widen to `i64` so
/// spread / mid arithmetic and aggregation can't overflow and so signed
/// deltas are representable.
pub type Price = i64;

/// Share quantity. Wire format is `u32`; widened for the same reason as
/// [`Price`].
pub type Quantity = u64;

/// Nanoseconds since midnight (ITCH timestamp semantics).
pub type Timestamp = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Bid,
    Ask,
}

impl Side {
    #[inline]
    pub fn opposite(self) -> Self {
        match self {
            Side::Bid => Side::Ask,
            Side::Ask => Side::Bid,
        }
    }
}

/// A resting order in the book.
#[derive(Debug, Clone, Copy)]
pub struct Order {
    pub id: OrderId,
    pub side: Side,
    pub price: Price,
    pub qty: Quantity,
    pub ts: Timestamp,
}

/// A trade synthesized when an execution message hits a resting order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trade {
    pub maker_id: OrderId,
    pub price: Price,
    pub qty: Quantity,
    pub ts: Timestamp,
}

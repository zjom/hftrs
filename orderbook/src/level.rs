//! A single price level.
//!
//! Each level owns the head and tail of a FIFO queue of resting orders at
//! that price (orders are stored in the [`crate::pool::Pool`]). It also
//! caches the aggregate quantity and order count so top-of-book and depth
//! queries don't have to walk the list.

use crate::pool::{Index, NIL};
use crate::types::Quantity;

#[derive(Debug, Clone, Copy)]
pub struct Level {
    /// Oldest order — gets filled first.
    pub head: Index,
    /// Newest order — new arrivals splice in here.
    pub tail: Index,
    pub total_qty: Quantity,
    pub order_count: u32,
}

impl Level {
    pub const fn empty() -> Self {
        Self {
            head: NIL,
            tail: NIL,
            total_qty: 0,
            order_count: 0,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head == NIL
    }
}

impl Default for Level {
    fn default() -> Self {
        Self::empty()
    }
}

//! The order book.
//!
//! ## Layout
//!
//! ```text
//!   bids:   BTreeMap<Price, Level>   // best bid is the rightmost key
//!   asks:   BTreeMap<Price, Level>   // best ask is the leftmost key
//!   orders: HashMap<OrderId, Index>  // O(1) cancel / execute
//!   pool:   Pool                     // arena of OrderNodes
//! ```
//!
//! `BTreeMap` is the conservative default — it's O(log n) on inserts and
//! removes and gives ordered iteration for free. For a fixed tick range
//! (e.g. a single NMS issue with a known price band) a vector indexed by
//! `(price - min) / tick_size` would give O(1) level access; the trait
//! surface here is narrow enough that swapping that in is a one-file
//! change. See `README.md` for benchmark plans comparing the two.

use std::collections::{BTreeMap, HashMap};

use crate::error::{BookError, Result};
use crate::level::Level;
use crate::pool::{Index, NIL, Node, Pool};
use crate::types::{Order, OrderId, Price, Quantity, Side, Timestamp, Trade};

#[derive(Debug)]
pub struct OrderBook {
    bids: BTreeMap<Price, Level>,
    asks: BTreeMap<Price, Level>,
    orders: HashMap<OrderId, Index>,
    pool: Pool<Order>,
}

impl OrderBook {
    /// Default arena and hashmap capacity. Tune via [`with_capacity`].
    pub fn new() -> Self {
        Self::with_capacity(1 << 16)
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            orders: HashMap::with_capacity(cap),
            pool: Pool::with_capacity(cap),
        }
    }

    // ---- ITCH event handlers ------------------------------------------------

    /// Add Order — ITCH `A` (no MPID) and `F` (with MPID). Inserts at the
    /// tail of its price level so time priority is preserved.
    pub fn add(&mut self, order: Order) -> Result<()> {
        if self.orders.contains_key(&order.id) {
            return Err(BookError::DuplicateOrder(order.id));
        }
        let Order {
            id,
            side,
            price,
            qty,
            ..
        } = order;

        // Snapshot the current tail (if any) before we touch the pool, so we
        // can wire up the linked list without overlapping borrows.
        let prev = self
            .side_levels(side)
            .get(&price)
            .map(|l| l.tail)
            .unwrap_or(NIL);

        let idx = self.pool.alloc(Node {
            order,
            prev,
            next: NIL,
        });

        if prev != NIL {
            self.pool.get_mut(prev).next = idx;
        }

        let levels = self.side_levels_mut(side);
        let level = levels.entry(price).or_insert_with(Level::empty);
        if level.head == NIL {
            level.head = idx;
        }
        level.tail = idx;
        level.total_qty += qty;
        level.order_count += 1;

        self.orders.insert(id, idx);
        Ok(())
    }

    /// Order Executed — ITCH `E`. The trade prints at the resting price.
    pub fn execute(&mut self, id: OrderId, qty: Quantity, ts: Timestamp) -> Result<Trade> {
        self.execute_inner(id, qty, ts, None)
    }

    /// Order Executed With Price — ITCH `C`. Trade prints at an explicit
    /// price (cross / auction prints, occasional through-the-book fills).
    pub fn execute_at(
        &mut self,
        id: OrderId,
        qty: Quantity,
        price: Price,
        ts: Timestamp,
    ) -> Result<Trade> {
        self.execute_inner(id, qty, ts, Some(price))
    }

    fn execute_inner(
        &mut self,
        id: OrderId,
        qty: Quantity,
        ts: Timestamp,
        print_price: Option<Price>,
    ) -> Result<Trade> {
        let &idx = self.orders.get(&id).ok_or(BookError::UnknownOrder(id))?;

        // Decrement the resting order's qty in the arena and capture what we
        // need for the trade record + level update.
        let (resting_price, side, exhausted) = {
            let node = self.pool.get_mut(idx);
            if qty > node.order.qty {
                return Err(BookError::OverExecute {
                    id,
                    got: qty,
                    avail: node.order.qty,
                });
            }
            node.order.qty -= qty;
            (node.order.price, node.order.side, node.order.qty == 0)
        };

        // Update the level aggregate.
        self.side_levels_mut(side)
            .get_mut(&resting_price)
            .expect("level must exist for live order")
            .total_qty -= qty;

        if exhausted {
            self.unlink(idx);
        }

        Ok(Trade {
            maker_id: id,
            price: print_price.unwrap_or(resting_price),
            qty,
            ts,
        })
    }

    /// Order Cancel — ITCH `X`. Partial cancel; reduces resting qty.
    pub fn cancel(&mut self, id: OrderId, qty: Quantity) -> Result<()> {
        let &idx = self.orders.get(&id).ok_or(BookError::UnknownOrder(id))?;

        let (price, side, exhausted) = {
            let node = self.pool.get_mut(idx);
            if qty > node.order.qty {
                return Err(BookError::OverCancel {
                    id,
                    got: qty,
                    avail: node.order.qty,
                });
            }
            node.order.qty -= qty;
            (node.order.price, node.order.side, node.order.qty == 0)
        };

        self.side_levels_mut(side)
            .get_mut(&price)
            .expect("level must exist for live order")
            .total_qty -= qty;

        if exhausted {
            self.unlink(idx);
        }
        Ok(())
    }

    /// Order Delete — ITCH `D`. Removes the entire order regardless of qty.
    pub fn delete(&mut self, id: OrderId) -> Result<()> {
        let &idx = self.orders.get(&id).ok_or(BookError::UnknownOrder(id))?;
        self.unlink(idx);
        Ok(())
    }

    /// Order Replace — ITCH `U`. Cancels the old order and inserts a new one
    /// with a fresh ID at the *tail* of its (possibly different) price
    /// level — i.e. it loses time priority. Side is inherited from the old
    /// order; ITCH replace messages don't carry a side.
    pub fn replace(
        &mut self,
        old_id: OrderId,
        new_id: OrderId,
        new_price: Price,
        new_qty: Quantity,
        ts: Timestamp,
    ) -> Result<()> {
        let &idx = self
            .orders
            .get(&old_id)
            .ok_or(BookError::UnknownOrder(old_id))?;
        let side = self.pool.get(idx).order.side;
        self.unlink(idx);
        self.add(Order {
            id: new_id,
            side,
            price: new_price,
            qty: new_qty,
            ts,
        })
    }

    // ---- Queries ------------------------------------------------------------

    /// Best bid as `(price, total qty at price)`. `O(log n)` on the BTreeMap.
    pub fn best_bid(&self) -> Option<(Price, Quantity)> {
        self.bids.iter().next_back().map(|(p, l)| (*p, l.total_qty))
    }

    /// Best ask as `(price, total qty at price)`. `O(log n)` on the BTreeMap.
    pub fn best_ask(&self) -> Option<(Price, Quantity)> {
        self.asks.iter().next().map(|(p, l)| (*p, l.total_qty))
    }

    pub fn spread(&self) -> Option<Price> {
        let (b, _) = self.best_bid()?;
        let (a, _) = self.best_ask()?;
        Some(a - b)
    }

    pub fn mid(&self) -> Option<Price> {
        let (b, _) = self.best_bid()?;
        let (a, _) = self.best_ask()?;
        Some((a + b) / 2)
    }

    /// Top `n` levels of depth on each side, near to far.
    pub fn depth(&self, n: usize) -> (Vec<(Price, Quantity)>, Vec<(Price, Quantity)>) {
        let bids = self
            .bids
            .iter()
            .rev()
            .take(n)
            .map(|(p, l)| (*p, l.total_qty))
            .collect();
        let asks = self
            .asks
            .iter()
            .take(n)
            .map(|(p, l)| (*p, l.total_qty))
            .collect();
        (bids, asks)
    }

    pub fn len(&self) -> usize {
        self.orders.len()
    }

    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    // ---- Internals ----------------------------------------------------------

    #[inline]
    fn side_levels(&self, side: Side) -> &BTreeMap<Price, Level> {
        match side {
            Side::Bid => &self.bids,
            Side::Ask => &self.asks,
        }
    }

    #[inline]
    fn side_levels_mut(&mut self, side: Side) -> &mut BTreeMap<Price, Level> {
        match side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        }
    }

    /// Splice an order out of its price level and free its slot.
    ///
    /// Safe to call when the node's `qty` has already been decremented to
    /// zero (the execute / full-cancel paths) — the level's `total_qty` is
    /// updated by the *remaining* qty on the node, which will be zero in
    /// that case.
    fn unlink(&mut self, idx: Index) {
        let node = *self.pool.get(idx);
        let remaining_qty = node.order.qty;
        let side = node.order.side;
        let price = node.order.price;

        // Splice out of the doubly linked list.
        if node.prev != NIL {
            self.pool.get_mut(node.prev).next = node.next;
        }
        if node.next != NIL {
            self.pool.get_mut(node.next).prev = node.prev;
        }

        let levels = self.side_levels_mut(side);
        let level_now_empty = {
            let level = levels
                .get_mut(&price)
                .expect("level must exist for live order");
            if level.head == idx {
                level.head = node.next;
            }
            if level.tail == idx {
                level.tail = node.prev;
            }
            level.total_qty = level.total_qty.saturating_sub(remaining_qty);
            level.order_count -= 1;
            level.is_empty()
        };
        if level_now_empty {
            levels.remove(&price);
        }

        self.orders.remove(&node.order.id);
        self.pool.free(idx);
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

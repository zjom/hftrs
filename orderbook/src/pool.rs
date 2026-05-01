//! Slab arena for order nodes.
//!
//! ## Why an arena rather than `Box<Node>`?
//!
//! - **No allocator pressure on the hot path.** Pre-sizing with
//!   [`Pool::with_capacity`] amortizes allocation up front; freed slots come
//!   back through an intrusive free list, so steady-state behavior is two
//!   loads and a store per allocation.
//! - **Compact indices.** Slots are addressed by [`Index`] (`u32`), half the
//!   width of a pointer. The `OrderId -> Index` map is denser as a result.
//! - **Locality.** Orders allocated close together in time tend to occupy
//!   adjacent slots, which helps when sweeping a price level.
//!
//! The doubly-linked-list pointers (`prev`, `next`) live inside [`Node`], so
//! splicing an order out of its price level is O(1) with no extra
//! indirection.

pub type Index = u32;

/// Sentinel for "no neighbor" / end-of-list.
pub const NIL: Index = u32::MAX;

#[derive(Debug, Clone, Copy)]
pub struct Node<T> {
    pub order: T,
    pub prev: Index,
    pub next: Index,
}

#[derive(Debug, Clone, Copy)]
enum Slot<T> {
    Occupied(Node<T>),
    Free(Index), // next free slot, or NIL
}

#[derive(Debug)]
pub struct Pool<T> {
    slots: Vec<Slot<T>>,
    free_head: Index,
}

impl<T> Pool<T> {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            slots: Vec::with_capacity(cap),
            free_head: NIL,
        }
    }

    pub fn alloc(&mut self, node: Node<T>) -> Index {
        if self.free_head != NIL {
            let idx = self.free_head;
            // SAFETY-ish: the free list only ever points at `Slot::Free`.
            if let Slot::Free(next) = self.slots[idx as usize] {
                self.free_head = next;
                self.slots[idx as usize] = Slot::Occupied(node);
                return idx;
            }
            unreachable!("free list pointed at occupied slot {}", idx);
        }
        let idx = self.slots.len() as Index;
        debug_assert!(idx != NIL, "pool exhausted (slot count hit u32::MAX)");
        self.slots.push(Slot::Occupied(node));
        idx
    }

    pub fn free(&mut self, idx: Index) {
        debug_assert!(matches!(self.slots[idx as usize], Slot::Occupied(_)));
        self.slots[idx as usize] = Slot::Free(self.free_head);
        self.free_head = idx;
    }

    #[inline]
    pub fn get(&self, idx: Index) -> &Node<T> {
        match &self.slots[idx as usize] {
            Slot::Occupied(n) => n,
            Slot::Free(_) => unreachable!("access to freed slot {}", idx),
        }
    }

    #[inline]
    pub fn get_mut(&mut self, idx: Index) -> &mut Node<T> {
        match &mut self.slots[idx as usize] {
            Slot::Occupied(n) => n,
            Slot::Free(_) => unreachable!("access to freed slot {}", idx),
        }
    }

    /// Number of slots ever allocated (including currently freed). Useful for
    /// capacity tuning during benchmarks.
    pub fn capacity_used(&self) -> usize {
        self.slots.len()
    }
}

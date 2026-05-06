use crate::OrderBook;
use itch5::messages::Symbol;
pub trait Registry {
    fn new() -> Self;
    fn with_capacity(cap: usize) -> Self;
    fn get_symbol(&self, locate: u16) -> Option<&Symbol>;
    fn get(&self, locate: u16) -> Option<&OrderBook>;
    fn get_mut(&mut self, locate: u16) -> Option<&mut OrderBook>;
    fn register(&mut self, locate: u16, symbol: &Symbol);
    fn len(&self) -> usize;
    fn iter(&self) -> impl Iterator<Item = (u16, &Symbol, &OrderBook)>;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

mod hashmap;
mod vec;
pub use hashmap::HashMapRegistry;
pub use vec::VecRegistry;

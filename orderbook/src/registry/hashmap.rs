use std::collections::HashMap;

use itch5::messages::Symbol;

use crate::{OrderBook, registry::Registry};

const DEFAULT_CAPACITY: usize = 1 << 16;

pub struct HashMapRegistry {
    /// Stock locate X OrderBook
    books: HashMap<u16, OrderBook>,
    /// Stock locate X Symbol
    symbols: HashMap<u16, Symbol>,
}

impl HashMapRegistry {
    #[inline]
    pub fn new() -> HashMapRegistry {
        Self {
            books: HashMap::with_capacity(DEFAULT_CAPACITY),
            symbols: HashMap::with_capacity(DEFAULT_CAPACITY),
        }
    }
    #[inline]
    pub fn with_capacity(cap: usize) -> HashMapRegistry {
        Self {
            books: HashMap::with_capacity(cap),
            symbols: HashMap::with_capacity(cap),
        }
    }

    /// Resolve a locate back to its ASCII symbol for logging.
    #[deprecated(note = "use [`Self::symbol`] instead")]
    #[inline]
    pub fn symbol_str(&self, locate: u16) -> Option<&str> {
        self.symbols.get(&locate).map(|s| s.as_str())
    }

    /// Resolve a locate back to its ASCII symbol for logging.
    #[inline]
    pub fn get_symbol(&self, locate: u16) -> Option<&Symbol> {
        self.symbols.get(&locate)
    }

    /// Create an entry for a stock locate.
    #[inline]
    pub fn register(&mut self, locate: u16, symbol: &Symbol) {
        self.symbols.insert(locate, *symbol);
        self.books.insert(locate, OrderBook::new());
    }

    #[inline]
    pub fn get(&self, locate: u16) -> Option<&OrderBook> {
        self.books.get(&locate)
    }

    #[inline]
    pub fn get_mut(&mut self, locate: u16) -> Option<&mut OrderBook> {
        self.books.get_mut(&locate)
    }
    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        Iter::new(self)
    }

    #[inline]
    pub fn len(&self) -> usize {
        debug_assert_eq!(self.books.len(), self.symbols.len());
        self.books.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Registry for HashMapRegistry {
    fn new() -> Self {
        Self::new()
    }
    fn with_capacity(cap: usize) -> Self {
        Self::with_capacity(cap)
    }
    fn get_symbol(&self, locate: u16) -> Option<&Symbol> {
        self.get_symbol(locate)
    }
    fn get(&self, locate: u16) -> Option<&OrderBook> {
        self.get(locate)
    }

    fn get_mut(&mut self, locate: u16) -> Option<&mut OrderBook> {
        self.get_mut(locate)
    }

    fn register(&mut self, locate: u16, symbol: &Symbol) {
        self.register(locate, symbol);
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn iter(&self) -> impl Iterator<Item = (u16, &Symbol, &OrderBook)> {
        self.iter()
    }
}

pub struct Iter<'a> {
    iter: std::collections::hash_map::Iter<'a, u16, Symbol>,
    registry: &'a HashMapRegistry,
}

impl<'a> Iter<'a> {
    #[inline]
    fn new(registry: &'a HashMapRegistry) -> Iter<'a> {
        Self {
            iter: registry.symbols.iter(),
            registry,
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = (u16, &'a Symbol, &'a OrderBook);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next()
            .map(|(locate, symb)| (*locate, symb, self.registry.get(*locate).unwrap()))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a> ExactSizeIterator for Iter<'a> {}

impl<'a> IntoIterator for &'a HashMapRegistry {
    type Item = (u16, &'a Symbol, &'a OrderBook);
    type IntoIter = Iter<'a>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Default for HashMapRegistry {
    fn default() -> Self {
        Self::new()
    }
}

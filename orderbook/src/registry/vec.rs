use itch5::messages::Symbol;

use crate::{OrderBook, registry::Registry};

const MAX_CAPACITY: usize = u16::MAX as usize;

pub struct VecRegistry {
    /// vec of [`OrderBook`] indexed by stock locate
    /// same length and capacity as [`Self::symbols`]
    books: Vec<Option<OrderBook>>,
    /// vec of [`Symbol`] indexed by stock locate
    /// same length and capacity as [`Self::books`]
    symbols: Vec<Option<Symbol>>,
}

impl VecRegistry {
    /// Creates a new [`VecRegistry`] with max capacity (1<<16)
    #[inline]
    pub fn new() -> Self {
        let mut books = Vec::with_capacity(MAX_CAPACITY);
        books.resize_with(MAX_CAPACITY, Default::default);

        let mut symbols = Vec::with_capacity(MAX_CAPACITY);
        symbols.resize_with(MAX_CAPACITY, Default::default);
        Self { books, symbols }
    }

    #[inline]
    pub fn with_capacity(cap: usize) -> Self {
        let mut books = Vec::with_capacity(cap);
        books.resize_with(cap, Default::default);

        let mut symbols = Vec::with_capacity(cap);
        symbols.resize_with(cap, Default::default);
        Self { books, symbols }
    }

    /// Resizes the `books` and `symbols` in-place so that `len` is equal to `new_len`.
    /// Panics if `new_len` is greater than 1<<16
    #[inline]
    pub fn resize(&mut self, new_len: usize) {
        assert!(new_len <= MAX_CAPACITY);
        self.books.resize_with(new_len, Default::default);
        self.symbols.resize_with(new_len, Default::default);
    }

    /// Resolve a locate back to its ASCII symbol for logging.
    #[inline]
    pub fn get_symbol(&self, locate: u16) -> Option<&Symbol> {
        self.symbols.get(locate as usize).and_then(Option::as_ref)
    }

    /// Create an entry for a stock locate.
    /// Resizes underlying vectors if needed.
    #[inline]
    pub fn register(&mut self, locate: u16, symbol: &Symbol) {
        if locate as usize >= self.len() {
            self.resize(locate as usize);
        }
        self.symbols[locate as usize] = Some(*symbol);
        self.books[locate as usize] = Some(OrderBook::new());
    }

    /// Get the [`OrderBook`] for a stock locate if exists.
    #[inline]
    pub fn get(&self, locate: u16) -> Option<&OrderBook> {
        self.books.get(locate as usize).and_then(Option::as_ref)
    }

    /// Get the [`OrderBook`] for a stock locate if exists.
    #[inline]
    pub fn get_mut(&mut self, locate: u16) -> Option<&mut OrderBook> {
        self.books.get_mut(locate as usize).and_then(Option::as_mut)
    }

    /// Returns the length of the underlying vectors.
    #[inline]
    pub fn len(&self) -> usize {
        debug_assert_eq!(self.books.len(), self.symbols.len());
        self.books.len()
    }

    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        Iter::new(self)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Registry for VecRegistry {
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
    books: std::slice::Iter<'a, Option<OrderBook>>,
    symbols: std::slice::Iter<'a, Option<Symbol>>,
    locate: u16,
}

impl<'a> Iter<'a> {
    #[inline]
    pub fn new(registry: &'a VecRegistry) -> Self {
        Iter {
            books: registry.books.iter(),
            symbols: registry.symbols.iter(),
            locate: 0,
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = (u16, &'a Symbol, &'a OrderBook);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Advance both iterators. If either is exhausted, iteration ends.
            let book_opt = self.books.next()?;
            let symbol_opt = self.symbols.next()?;

            let current_locate = self.locate;
            self.locate += 1;

            // Only yield if both a Symbol and OrderBook exist at this index
            if let (Some(symbol), Some(book)) = (symbol_opt.as_ref(), book_opt.as_ref()) {
                return Some((current_locate, symbol, book));
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.books.size_hint()
    }
}

impl<'a> ExactSizeIterator for Iter<'a> {}

impl<'a> IntoIterator for &'a VecRegistry {
    type Item = (u16, &'a Symbol, &'a OrderBook);
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            books: self.books.iter(),
            symbols: self.symbols.iter(),
            locate: 0,
        }
    }
}

impl Default for VecRegistry {
    fn default() -> Self {
        Self::new()
    }
}

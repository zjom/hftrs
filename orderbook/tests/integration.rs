use orderbook::{Order, OrderBook, Side};

fn ord(id: u64, side: Side, price: i64, qty: u64) -> Order {
    Order {
        id,
        side,
        price,
        qty,
        ts: 0,
    }
}

#[test]
fn add_and_query_top_of_book() {
    let mut b = OrderBook::new();
    b.add(ord(1, Side::Bid, 99, 100)).unwrap();
    b.add(ord(2, Side::Bid, 100, 50)).unwrap();
    b.add(ord(3, Side::Ask, 101, 30)).unwrap();
    b.add(ord(4, Side::Ask, 102, 80)).unwrap();

    assert_eq!(b.best_bid(), Some((100, 50)));
    assert_eq!(b.best_ask(), Some((101, 30)));
    assert_eq!(b.spread(), Some(1));
    assert_eq!(b.mid(), Some(100)); // (100 + 101) / 2 = 100 with i64 trunc
}

#[test]
fn fifo_within_price_level() {
    let mut b = OrderBook::new();
    b.add(ord(1, Side::Bid, 100, 100)).unwrap();
    b.add(ord(2, Side::Bid, 100, 200)).unwrap();
    b.add(ord(3, Side::Bid, 100, 50)).unwrap();

    // Order 1 (oldest) gets filled first — it sits at the head.
    let trade = b.execute(1, 100, 0).unwrap();
    assert_eq!(trade.maker_id, 1);
    assert_eq!(trade.qty, 100);

    // Aggregate updates and order 1 is gone.
    assert_eq!(b.best_bid(), Some((100, 250)));
    assert!(b.delete(1).is_err()); // already removed

    // Order 2 is now the head; partial execute leaves it resting.
    b.execute(2, 50, 0).unwrap();
    assert_eq!(b.best_bid(), Some((100, 200)));
}

#[test]
fn delete_clears_empty_level() {
    let mut b = OrderBook::new();
    b.add(ord(1, Side::Bid, 100, 100)).unwrap();
    b.delete(1).unwrap();
    assert_eq!(b.best_bid(), None);
}

#[test]
fn cancel_partial_then_delete() {
    let mut b = OrderBook::new();
    b.add(ord(1, Side::Bid, 100, 100)).unwrap();
    b.cancel(1, 30).unwrap();
    assert_eq!(b.best_bid(), Some((100, 70)));
    b.delete(1).unwrap();
    assert_eq!(b.best_bid(), None);
}

#[test]
fn cancel_to_zero_unlinks_order() {
    let mut b = OrderBook::new();
    b.add(ord(1, Side::Bid, 100, 100)).unwrap();
    b.add(ord(2, Side::Bid, 100, 50)).unwrap();
    b.cancel(1, 100).unwrap();
    // Order 1 fully cancelled; order 2 is alone at this level.
    assert_eq!(b.best_bid(), Some((100, 50)));
    assert!(b.delete(1).is_err());
}

#[test]
fn replace_loses_time_priority() {
    let mut b = OrderBook::new();
    b.add(ord(1, Side::Bid, 100, 100)).unwrap();
    b.add(ord(2, Side::Bid, 100, 50)).unwrap();

    // Replace order 1 with new id 3; it goes to the tail.
    b.replace(1, 3, 100, 100, 0).unwrap();

    // Order 2 is now the oldest at this price.
    let trade = b.execute(2, 50, 0).unwrap();
    assert_eq!(trade.maker_id, 2);
    assert_eq!(b.best_bid(), Some((100, 100)));
}

#[test]
fn duplicate_order_rejected() {
    let mut b = OrderBook::new();
    b.add(ord(1, Side::Bid, 100, 100)).unwrap();
    assert!(b.add(ord(1, Side::Bid, 101, 50)).is_err());
}

#[test]
fn over_execute_rejected() {
    let mut b = OrderBook::new();
    b.add(ord(1, Side::Bid, 100, 100)).unwrap();
    assert!(b.execute(1, 101, 0).is_err());
    // Original order untouched.
    assert_eq!(b.best_bid(), Some((100, 100)));
}

#[test]
fn execute_at_price_uses_print_price() {
    let mut b = OrderBook::new();
    b.add(ord(1, Side::Bid, 100, 100)).unwrap();
    let trade = b.execute_at(1, 50, 99, 0).unwrap();
    assert_eq!(trade.price, 99); // cross / auction print price
    assert_eq!(b.best_bid(), Some((100, 50)));
}

#[test]
fn depth_returns_top_n_per_side() {
    let mut b = OrderBook::new();
    for i in 0..5 {
        b.add(ord(i, Side::Bid, 100 - i as i64, 10)).unwrap();
        b.add(ord(100 + i, Side::Ask, 101 + i as i64, 10)).unwrap();
    }
    let (bids, asks) = b.depth(3);
    assert_eq!(bids, vec![(100, 10), (99, 10), (98, 10)]);
    assert_eq!(asks, vec![(101, 10), (102, 10), (103, 10)]);
}

//! ITCH 5.0 message dispatch + per-run book maintenance.
//!
//! [`MessageHandler`] implements [`itch5::MessageHandler`] and threads
//! incoming events into a [`Registry`] of [`OrderBook`]s. The [`run`]
//! function owns the receive-side loop: it pulls [`Datagram`]s from the
//! moldudp client, validates packet integrity, requests retransmissions
//! when needed, and feeds the parser.
//!
//! [`OrderBook`]: orderbook::OrderBook

use itch5::messages::{
    AddOrderNoMPIDAttribution, AddOrderWithMPIDAttribution, BuySellIndicator, OrderCancel,
    OrderDelete, OrderExecuted, OrderExecutedWithPrice, OrderReplace, StockDirectory, Symbol,
};
use moldudp::{
    Datagram, FromBytes, Packet, PacketKind, Receiver, RetransmissionPacket, RetransmissionRequest,
    Sender,
};
use orderbook::registry::Registry;
use orderbook::{Order, Side};
use std::collections::HashSet;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Shared handle to the message handler. The receive loop owns the underlying
/// `Mutex` for the duration of `parse_stream` on each packet; readers (e.g.
/// the TUI thread) acquire the same `Mutex` to snapshot registry state. Lock
/// contention on the hot path is bounded by the TUI's redraw cadence.
pub type SharedHandler<R> = Arc<Mutex<MessageHandler<R>>>;

/// Drives the receive loop until end-of-session, shutdown, or a 5 s recv
/// timeout. The handler is shared via [`SharedHandler`] so external readers
/// can snapshot it; the lock is acquired briefly per packet only.
pub fn run<R: Registry>(
    handler: SharedHandler<R>,
    rx: Receiver<Datagram>,
    req_tx: Sender<RetransmissionRequest>,
    shutdown: Arc<AtomicBool>,
) {
    let client_start = Instant::now();
    let mut net = NetCounters::default();

    while let Ok(datagram) = rx.recv_timeout(Duration::from_secs(5)) {
        if shutdown.load(Ordering::Relaxed) {
            tracing::info!(
                "client shutdown requested after {} packets ({} heartbeats, {} rereqs)",
                net.packets_received,
                net.heartbeats,
                net.retransmission_requests,
            );
            break;
        }

        let packet = Packet::ref_from_bytes(datagram.bytes()).unwrap();
        let seq_num = packet.seq_num();
        let advertised = packet.msg_count();

        match packet.packet_kind() {
            PacketKind::Heartbeat => {
                net.heartbeats += 1;
                tracing::trace!("heartbeat (total={})", net.heartbeats);
                continue;
            }
            PacketKind::EndOfSession => {
                tracing::info!("end-of-session packet received at seq={seq_num}");
                break;
            }
            _ => {}
        }

        net.packets_received += 1;
        tracing::debug!(
            "packet received: seq={seq_num} msg_count={advertised} (total_packets={})",
            net.packets_received,
        );

        let actual = packet.iter().len();
        if actual != advertised as usize {
            net.truncated_packets += 1;
            tracing::warn!(
                "truncated packet at seq={seq_num}: advertised {advertised} messages but \
                 found {actual} (total truncated={}); sending retransmission request",
                net.truncated_packets,
            );
            request_retransmission(&req_tx, packet, &mut net);
        }

        tracing::trace!("parsing {actual} message(s) from seq={seq_num}");
        let mut h = handler.lock().expect("handler mutex poisoned");
        if let Err(e) = itch5::Parser::new(&packet.messages).parse_stream(&mut *h) {
            net.parse_errors += 1;
            tracing::error!(
                "parse error in packet seq={seq_num} (total_parse_errors={}): {e}",
                net.parse_errors,
            );
        }
    }

    let h = handler.lock().expect("handler mutex poisoned");
    tracing::info!(
        "client thread done in {:.2?}: {net} | {} | registered_symbols={}",
        client_start.elapsed(),
        h.stats,
        h.registry.len(),
    );
}

fn request_retransmission(
    req_tx: &Sender<RetransmissionRequest>,
    packet: &Packet,
    net: &mut NetCounters,
) {
    let rereq = RetransmissionPacket {
        msg_count: packet.msg_count().into(),
        seq_num: packet.seq_num().into(),
        session: *packet.session_ident_raw(),
    };
    match req_tx.try_send(RetransmissionRequest::new(rereq)) {
        Ok(()) => {
            net.retransmission_requests += 1;
            tracing::debug!(
                "retransmission request sent for seq={} (total_rereqs={})",
                packet.seq_num(),
                net.retransmission_requests,
            );
        }
        Err(e) => tracing::error!(
            "failed to enqueue retransmission request for seq={}: {e}",
            packet.seq_num(),
        ),
    }
}

/// Network-layer counters scoped to one [`run`] invocation. Owned by the
/// receive loop, not the handler — the handler's [`HandlerStats`] tracks
/// book-level outcomes only.
#[derive(Default)]
struct NetCounters {
    packets_received: u64,
    heartbeats: u64,
    retransmission_requests: u64,
    parse_errors: u64,
    truncated_packets: u64,
}

impl std::fmt::Display for NetCounters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "packets={} heartbeats={} rereqs={} truncated={} parse_errors={}",
            self.packets_received,
            self.heartbeats,
            self.retransmission_requests,
            self.truncated_packets,
            self.parse_errors,
        )
    }
}

/// Book-level outcome counters reported alongside the final summary and
/// included in the report payload.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandlerStats {
    pub orders_added: u64,
    pub orders_executed: u64,
    pub orders_cancelled: u64,
    pub orders_deleted: u64,
    pub orders_replaced: u64,
    pub stock_directory_msgs: u64,
    /// Orders that referenced an unregistered locate code (not in watch list).
    pub skipped_locate: u64,
}

impl std::fmt::Display for HandlerStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "added={} executed={} cancelled={} deleted={} replaced={} skipped_locate={}",
            self.orders_added,
            self.orders_executed,
            self.orders_cancelled,
            self.orders_deleted,
            self.orders_replaced,
            self.skipped_locate,
        )
    }
}

/// Stateful ITCH handler over a [`Registry`].
///
/// `symbols_to_watch = None` registers every stock-directory entry; `Some`
/// restricts the registry to a fixed allow-list (matched on packed-`u64`
/// symbol bytes for cheap equality).
pub struct MessageHandler<R: Registry> {
    registry: R,
    symbols_to_watch: Option<HashSet<u64>>,
    stats: HandlerStats,
    shutdown: Arc<AtomicBool>,
    /// Coarse counter so we only touch the shutdown atomic once per
    /// [`SHUTDOWN_POLL_MASK`] events.
    tick: u32,
}

/// Poll the shutdown flag every 256 events. Power-of-two mask for cheap
/// `& MASK` rather than `% N`.
const SHUTDOWN_POLL_MASK: u32 = 0xFF;

impl<R: Registry> MessageHandler<R> {
    pub fn new(symbols_to_watch: Option<Vec<Symbol>>, shutdown: Arc<AtomicBool>) -> Self {
        let symbols_to_watch = symbols_to_watch.map(|symbols| {
            tracing::debug!("creating MessageHandler for {} symbol(s)", symbols.len());
            symbols.iter().map(|s| s.to_u64()).collect()
        });
        if symbols_to_watch.is_none() {
            tracing::debug!("creating MessageHandler for all symbols");
        }
        Self {
            registry: R::new(),
            symbols_to_watch,
            stats: HandlerStats::default(),
            shutdown,
            tick: 0,
        }
    }

    #[inline]
    pub const fn registry(&self) -> &R {
        &self.registry
    }

    #[inline]
    pub const fn stats(&self) -> &HandlerStats {
        &self.stats
    }

    #[inline]
    fn should_stop(&mut self) -> ControlFlow<()> {
        self.tick = self.tick.wrapping_add(1);
        if self.tick & SHUTDOWN_POLL_MASK == 0 && self.shutdown.load(Ordering::Relaxed) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    /// Shared body of `on_add_order_*`. The two ITCH variants only differ
    /// in their MPID field (which the book ignores), so they fan in here.
    #[inline]
    fn apply_add_order(&mut self, kind: &'static str, msg: &dyn AddOrderLike) {
        let locate = msg.stock_locate();
        let order_ref = msg.order_reference_number();
        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            tracing::trace!("{kind}: skipping untracked locate={locate}");
            return;
        };
        let Some(side) = parse_side(msg.buy_sell_indicator()) else {
            tracing::error!(
                "{kind}: unknown buy/sell indicator for order_ref={order_ref} locate={locate}",
            );
            return;
        };
        let price = msg.price_i64();
        let shares = msg.shares();
        tracing::debug!(
            "{kind}: order_ref={order_ref} side={side:?} price={price} qty={shares} \
             ts={} locate={locate}",
            msg.timestamp_naive(),
        );
        match book.add(Order {
            id: order_ref,
            side,
            price,
            qty: shares as u64,
            ts: msg.timestamp_u64(),
        }) {
            Ok(()) => self.stats.orders_added += 1,
            Err(e) => {
                tracing::error!("{kind}: failed to add order_ref={order_ref} locate={locate}: {e}")
            }
        }
    }
}

/// Shared shape of `AddOrder*` messages, used by [`MessageHandler::apply_add_order`]
/// to fan the MPID and no-MPID variants into a single body.
trait AddOrderLike {
    fn stock_locate(&self) -> u16;
    fn order_reference_number(&self) -> u64;
    fn buy_sell_indicator(&self) -> BuySellIndicator;
    fn price_i64(&self) -> i64;
    fn shares(&self) -> u32;
    fn timestamp_u64(&self) -> u64;
    fn timestamp_naive(&self) -> Box<dyn std::fmt::Display>;
}

impl AddOrderLike for AddOrderNoMPIDAttribution {
    fn stock_locate(&self) -> u16 {
        self.stock_locate()
    }
    fn order_reference_number(&self) -> u64 {
        self.order_reference_number()
    }
    fn buy_sell_indicator(&self) -> BuySellIndicator {
        self.buy_sell_indicator()
    }
    fn price_i64(&self) -> i64 {
        self.price().into_i64()
    }
    fn shares(&self) -> u32 {
        self.shares()
    }
    fn timestamp_u64(&self) -> u64 {
        self.timestamp().to_u64()
    }
    fn timestamp_naive(&self) -> Box<dyn std::fmt::Display> {
        Box::new(self.timestamp().to_naive_time())
    }
}

impl AddOrderLike for AddOrderWithMPIDAttribution {
    fn stock_locate(&self) -> u16 {
        self.stock_locate()
    }
    fn order_reference_number(&self) -> u64 {
        self.order_reference_number()
    }
    fn buy_sell_indicator(&self) -> BuySellIndicator {
        self.buy_sell_indicator()
    }
    fn price_i64(&self) -> i64 {
        self.price().into_i64()
    }
    fn shares(&self) -> u32 {
        self.shares()
    }
    fn timestamp_u64(&self) -> u64 {
        self.timestamp().to_u64()
    }
    fn timestamp_naive(&self) -> Box<dyn std::fmt::Display> {
        Box::new(self.timestamp().to_naive_time())
    }
}

#[inline]
fn parse_side(ind: BuySellIndicator) -> Option<Side> {
    match ind {
        BuySellIndicator::Buy => Some(Side::Bid),
        BuySellIndicator::Sell => Some(Side::Ask),
        BuySellIndicator::Unknown(_) => None,
    }
}

impl<R: Registry> itch5::MessageHandler for MessageHandler<R> {
    fn on_stock_directory(&mut self, msg: &StockDirectory) -> ControlFlow<()> {
        self.should_stop()?;
        self.stats.stock_directory_msgs += 1;
        let stock = msg.stock();
        if self
            .symbols_to_watch
            .as_ref()
            .is_none_or(|r| r.contains(&stock.to_u64()))
        {
            tracing::info!(
                "registering symbol {} with locate={} (total_registered={})",
                stock.as_str(),
                msg.stock_locate(),
                self.registry.len() + 1,
            );
            self.registry.register(msg.stock_locate(), stock);
        } else {
            tracing::trace!(
                "ignoring stock-directory entry for {} (not in watch list)",
                stock.as_str(),
            );
        }
        ControlFlow::Continue(())
    }

    fn on_add_order_no_mpid_attribution(
        &mut self,
        msg: &AddOrderNoMPIDAttribution,
    ) -> ControlFlow<()> {
        self.should_stop()?;
        self.apply_add_order("add_order (no-mpid)", msg);
        ControlFlow::Continue(())
    }

    fn on_add_order_with_mpid_attribution(
        &mut self,
        msg: &AddOrderWithMPIDAttribution,
    ) -> ControlFlow<()> {
        self.should_stop()?;
        self.apply_add_order("add_order (mpid)", msg);
        ControlFlow::Continue(())
    }

    fn on_order_executed(&mut self, msg: &OrderExecuted) -> ControlFlow<()> {
        self.should_stop()?;
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();
        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            tracing::trace!("order_executed: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };
        tracing::debug!(
            "order_executed: order_ref={order_ref} shares={} ts={} locate={locate}",
            msg.executed_shares(),
            msg.timestamp().to_naive_time(),
        );
        match book.execute(
            order_ref,
            msg.executed_shares() as u64,
            msg.timestamp().to_u64(),
        ) {
            Ok(_) => self.stats.orders_executed += 1,
            Err(e) => {
                tracing::error!(
                    "order_executed: failed for order_ref={order_ref} locate={locate}: {e}"
                )
            }
        }
        ControlFlow::Continue(())
    }

    fn on_order_executed_with_price(&mut self, msg: &OrderExecutedWithPrice) -> ControlFlow<()> {
        self.should_stop()?;
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();
        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            tracing::trace!("order_executed_with_price: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };
        tracing::debug!(
            "order_executed_with_price: order_ref={order_ref} shares={} price={} ts={} \
             locate={locate}",
            msg.executed_shares(),
            msg.execution_price().into_i64(),
            msg.timestamp().to_naive_time(),
        );
        match book.execute_at(
            order_ref,
            msg.executed_shares() as u64,
            msg.execution_price().into_i64(),
            msg.timestamp().to_u64(),
        ) {
            Ok(_) => self.stats.orders_executed += 1,
            Err(e) => tracing::error!(
                "order_executed_with_price: failed for order_ref={order_ref} \
                 locate={locate}: {e}"
            ),
        }
        ControlFlow::Continue(())
    }

    fn on_order_cancel(&mut self, msg: &OrderCancel) -> ControlFlow<()> {
        self.should_stop()?;
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();
        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            tracing::trace!("order_cancel: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };
        tracing::debug!(
            "order_cancel: order_ref={order_ref} cancelled_shares={} locate={locate}",
            msg.cancelled_shares(),
        );
        match book.cancel(order_ref, msg.cancelled_shares() as u64) {
            Ok(()) => self.stats.orders_cancelled += 1,
            Err(e) => {
                tracing::error!(
                    "order_cancel: failed for order_ref={order_ref} locate={locate}: {e}"
                )
            }
        }
        ControlFlow::Continue(())
    }

    fn on_order_delete(&mut self, msg: &OrderDelete) -> ControlFlow<()> {
        self.should_stop()?;
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();
        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            tracing::trace!("order_delete: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };
        tracing::debug!("order_delete: order_ref={order_ref} locate={locate}");
        match book.delete(order_ref) {
            Ok(()) => self.stats.orders_deleted += 1,
            Err(e) => {
                tracing::error!(
                    "order_delete: failed for order_ref={order_ref} locate={locate}: {e}"
                )
            }
        }
        ControlFlow::Continue(())
    }

    fn on_order_replace(&mut self, msg: &OrderReplace) -> ControlFlow<()> {
        self.should_stop()?;
        let og_order_ref = msg.original_order_reference_number();
        let new_order_ref = msg.new_order_reference_number();
        let locate = msg.stock_locate();
        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            tracing::trace!(
                "order_replace: skipping untracked locate={locate} \
                 (og={og_order_ref} new={new_order_ref})",
            );
            return ControlFlow::Continue(());
        };
        tracing::debug!(
            "order_replace: og={og_order_ref} new={new_order_ref} price={} shares={} ts={} \
             locate={locate}",
            msg.price().into_i64(),
            msg.shares(),
            msg.timestamp().to_naive_time(),
        );
        match book.replace(
            og_order_ref,
            new_order_ref,
            msg.price().into_i64(),
            msg.shares() as u64,
            msg.timestamp().to_u64(),
        ) {
            Ok(()) => self.stats.orders_replaced += 1,
            Err(e) => tracing::error!(
                "order_replace: failed to replace og={og_order_ref} with new={new_order_ref} \
                 locate={locate}: {e}"
            ),
        }
        ControlFlow::Continue(())
    }
}

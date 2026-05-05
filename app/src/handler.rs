use itch5::messages::*;
use moldudp::{
    Datagram, FromBytes, Packet, PacketKind, Receiver, RetransmissionPacket, RetransmissionRequest,
    Sender,
};
use orderbook::registry::{Registry, VecRegistry};
use orderbook::{Order, Side};
use std::collections::HashSet;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub fn start_handler(
    symbols_to_watch: Option<Vec<Symbol>>,
    rx: Receiver<Datagram>,
    req_tx: Sender<RetransmissionRequest>,
    shutdown: Arc<AtomicBool>,
) -> MessageHandler<VecRegistry> {
    let client_start = Instant::now();

    let mut handler = match symbols_to_watch {
        Some(ss) => MessageHandler::<VecRegistry>::with_symbols(ss, Arc::clone(&shutdown)),
        None => MessageHandler::new(Arc::clone(&shutdown)),
    };

    // Per-run counters.
    let mut packets_received: u64 = 0;
    let mut heartbeats: u64 = 0;
    let mut retransmission_requests: u64 = 0;
    let mut parse_errors: u64 = 0;
    let mut truncated_packets: u64 = 0;

    while let Ok(datagram) = rx.recv_timeout(Duration::from_secs(5)) {
        if shutdown.load(Ordering::Relaxed) {
            log::info!(
                "client shutdown requested after {} packets ({} heartbeats, {} rereqs)",
                packets_received,
                heartbeats,
                retransmission_requests,
            );
            break;
        }

        let packet = Packet::ref_from_bytes(datagram.bytes()).unwrap();
        let msg_count = packet.msg_count();
        let seq_num = packet.seq_num();

        match packet.packet_kind() {
            PacketKind::Heartbeat => {
                heartbeats += 1;
                log::trace!("heartbeat (total={})", heartbeats);
                continue;
            }
            PacketKind::EndOfSession => {
                log::info!("end-of-session packet received at seq={}", seq_num);
                break;
            }
            _ => {}
        }

        packets_received += 1;
        log::debug!(
            "packet received: seq={} msg_count={} (total_packets={})",
            seq_num,
            msg_count,
            packets_received,
        );

        // Integrity check: does the payload actually contain the
        // advertised number of messages?
        let actual_msg_count = packet.iter().len();
        if actual_msg_count != msg_count as usize {
            truncated_packets += 1;
            log::warn!(
                "truncated packet at seq={}: advertised {} messages but found {} \
                         (total truncated={}); sending retransmission request",
                seq_num,
                msg_count,
                actual_msg_count,
                truncated_packets,
            );
            let rereq = RetransmissionPacket {
                msg_count: msg_count.into(),
                seq_num: seq_num.into(),
                session: *packet.session_ident_raw(),
            };
            match req_tx.try_send(RetransmissionRequest::new(rereq)) {
                Ok(()) => {
                    retransmission_requests += 1;
                    log::debug!(
                        "retransmission request sent for seq={} (total_rereqs={})",
                        seq_num,
                        retransmission_requests,
                    );
                }
                Err(e) => log::error!(
                    "failed to enqueue retransmission request for seq={}: {e}",
                    seq_num
                ),
            }
        }

        log::trace!(
            "parsing {} message(s) from seq={}",
            actual_msg_count,
            seq_num
        );
        if let Err(e) = itch5::Parser::new(&packet.messages).parse_stream(&mut handler) {
            parse_errors += 1;
            log::error!(
                "parse error in packet seq={} (total_parse_errors={}): {e}",
                seq_num,
                parse_errors,
            );
            continue;
        }
    }

    let elapsed = client_start.elapsed();
    log::info!(
        "client thread done in {:.2?}: packets={} heartbeats={} rereqs={} \
                 truncated={} parse_errors={} | orders_added={} executed={} cancelled={} \
                 deleted={} replaced={} registered_symbols={}",
        elapsed,
        packets_received,
        heartbeats,
        retransmission_requests,
        truncated_packets,
        parse_errors,
        handler.stats.orders_added,
        handler.stats.orders_executed,
        handler.stats.orders_cancelled,
        handler.stats.orders_deleted,
        handler.stats.orders_replaced,
        handler.registry.len(),
    );

    handler
}

/// Running totals for the client session, used in the final summary log line.
#[derive(Default)]
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

pub struct MessageHandler<R: Registry> {
    registry: R,
    symbols_to_watch: Option<HashSet<u64>>,
    stats: HandlerStats,
    shutdown: Arc<AtomicBool>,
    tick: u32,
}

impl<R: Registry> MessageHandler<R> {
    pub const fn registry(&self) -> &R {
        &self.registry
    }

    pub const fn stats(&self) -> &HandlerStats {
        &self.stats
    }
}

impl<R: Registry> MessageHandler<R> {
    fn new(shutdown: Arc<AtomicBool>) -> MessageHandler<R> {
        log::debug!("creating MessageHandler for all symbols");
        MessageHandler {
            registry: R::new(),
            symbols_to_watch: None,
            stats: HandlerStats::default(),
            shutdown: shutdown,
            tick: 0,
        }
    }

    #[inline]
    fn should_stop(&mut self) -> ControlFlow<()> {
        self.tick = self.tick.wrapping_add(1);
        if self.tick & 0xFF == 0 && self.shutdown.load(Ordering::Relaxed) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

impl MessageHandler<VecRegistry> {
    fn with_symbols(
        symbols: Vec<Symbol>,
        shutdown: Arc<AtomicBool>,
    ) -> MessageHandler<VecRegistry> {
        log::debug!("creating MessageHandler for {} symbol(s)", symbols.len());
        MessageHandler {
            registry: VecRegistry::new(),
            symbols_to_watch: Some(symbols.iter().map(|s| s.to_u64()).collect()),
            stats: HandlerStats::default(),
            shutdown: shutdown,
            tick: 0,
        }
    }
}

// impl MessageHandler<HashMapRegistry> {
//     fn with_symbols(symbols: Vec<Symbol>) -> MessageHandler<HashMapRegistry> {
//         log::debug!("creating MessageHandler for {} symbol(s)", symbols.len());
//         MessageHandler {
//             registry: HashMapRegistry::with_capacity(symbols.len()),
//             symbols_to_watch: Some(symbols.iter().map(|s| s.to_u64()).collect()),
//             stats: HandlerStats::default(),
//         }
//     }
// }

impl<R: Registry> itch5::MessageHandler for MessageHandler<R> {
    fn on_stock_directory(&mut self, msg: &StockDirectory) -> ControlFlow<()> {
        self.should_stop()?;
        self.stats.stock_directory_msgs += 1;
        let stock = msg.stock();
        if self
            .symbols_to_watch
            .as_ref()
            .map_or(true, |r| r.contains(&stock.to_u64()))
        {
            log::info!(
                "registering symbol {} with locate={} (total_registered={})",
                stock.as_str(),
                msg.stock_locate(),
                self.registry.len() + 1,
            );
            self.registry.register(msg.stock_locate(), stock);
        } else {
            log::trace!(
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
        let locate = msg.stock_locate();
        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            log::trace!("add_order (no-mpid): skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        let order_ref = msg.order_reference_number();
        let side = match msg.buy_sell_indicator() {
            BuySellIndicator::Buy => Side::Bid,
            BuySellIndicator::Sell => Side::Ask,
            BuySellIndicator::Unknown(unknown) => {
                log::error!(
                    "add_order (no-mpid): unknown buy/sell indicator '{unknown}' \
                     for order_ref={order_ref} locate={locate}"
                );
                return ControlFlow::Continue(());
            }
        };

        log::debug!(
            "add_order (no-mpid): order_ref={order_ref} side={side:?} \
             price={} qty={} ts={} locate={locate}",
            msg.price().into_i64(),
            msg.shares(),
            msg.timestamp().to_naive_time(),
        );

        match book.add(Order {
            id: order_ref,
            side,
            price: msg.price().into_i64(),
            qty: msg.shares() as u64,
            ts: msg.timestamp().to_u64(),
        }) {
            Ok(()) => self.stats.orders_added += 1,
            Err(e) => log::error!(
                "add_order (no-mpid): failed to add order_ref={order_ref} \
                 locate={locate}: {e}"
            ),
        }
        ControlFlow::Continue(())
    }

    fn on_add_order_with_mpid_attribution(
        &mut self,
        msg: &AddOrderWithMPIDAttribution,
    ) -> ControlFlow<()> {
        self.should_stop()?;
        let locate = msg.stock_locate();
        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            log::trace!("add_order (mpid): skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        let order_ref = msg.order_reference_number();
        let side = match msg.buy_sell_indicator() {
            BuySellIndicator::Buy => Side::Bid,
            BuySellIndicator::Sell => Side::Ask,
            BuySellIndicator::Unknown(unknown) => {
                log::error!(
                    "add_order (mpid): unknown buy/sell indicator '{unknown}' \
                     for order_ref={order_ref} locate={locate}"
                );
                return ControlFlow::Continue(());
            }
        };

        log::debug!(
            "add_order (mpid): order_ref={order_ref} side={side:?} \
             price={} qty={} ts={} locate={locate}",
            msg.price().into_i64(),
            msg.shares(),
            msg.timestamp().to_naive_time(),
        );

        match book.add(Order {
            id: order_ref,
            side,
            price: msg.price().into_i64(),
            qty: msg.shares() as u64,
            ts: msg.timestamp().to_u64(),
        }) {
            Ok(()) => self.stats.orders_added += 1,
            Err(e) => log::error!(
                "add_order (mpid): failed to add order_ref={order_ref} \
                 locate={locate}: {e}"
            ),
        }
        ControlFlow::Continue(())
    }

    fn on_order_executed(&mut self, msg: &OrderExecuted) -> ControlFlow<()> {
        self.should_stop()?;
        let order_ref = msg.order_reference_number();
        let locate = msg.stock_locate();

        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            log::trace!("order_executed: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        log::debug!(
            "order_executed: order_ref={order_ref} shares={} ts={} locate={locate}",
            msg.executed_shares(),
            msg.timestamp().to_naive_time(),
        );

        match book.execute(
            order_ref,
            msg.executed_shares() as u64,
            msg.timestamp().to_u64(),
        ) {
            Ok(_) => {
                self.stats.orders_executed += 1;
            }
            Err(e) => {
                log::error!("order_executed: failed for order_ref={order_ref} locate={locate}: {e}")
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
            log::trace!("order_executed_with_price: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        log::debug!(
            "order_executed_with_price: order_ref={order_ref} shares={} price={} ts={} locate={locate}",
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
            Ok(_) => {
                self.stats.orders_executed += 1;
            }
            Err(e) => log::error!(
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
            log::trace!("order_cancel: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        log::debug!(
            "order_cancel: order_ref={order_ref} cancelled_shares={} locate={locate}",
            msg.cancelled_shares(),
        );

        match book.cancel(order_ref, msg.cancelled_shares() as u64) {
            Ok(()) => self.stats.orders_cancelled += 1,
            Err(e) => {
                log::error!("order_cancel: failed for order_ref={order_ref} locate={locate}: {e}")
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
            log::trace!("order_delete: skipping untracked locate={locate}");
            return ControlFlow::Continue(());
        };

        log::debug!("order_delete: order_ref={order_ref} locate={locate}");

        match book.delete(order_ref) {
            Ok(()) => self.stats.orders_deleted += 1,
            Err(e) => {
                log::error!("order_delete: failed for order_ref={order_ref} locate={locate}: {e}")
            }
        }
        ControlFlow::Continue(())
    }

    fn on_order_replace(&mut self, msg: &OrderReplace) -> ControlFlow<()> {
        self.should_stop()?;
        let og_order_ref = msg.original_order_reference_number();
        let locate = msg.stock_locate();
        let new_order_ref = msg.new_order_reference_number();

        let Some(book) = self.registry.get_mut(locate) else {
            self.stats.skipped_locate += 1;
            log::trace!(
                "order_replace: skipping untracked locate={locate} \
                 (og={og_order_ref} new={new_order_ref})"
            );
            return ControlFlow::Continue(());
        };

        log::debug!(
            "order_replace: og={og_order_ref} new={new_order_ref} \
             price={} shares={} ts={} locate={locate}",
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
            Err(e) => log::error!(
                "order_replace: failed to replace og={og_order_ref} with \
                 new={new_order_ref} locate={locate}: {e}"
            ),
        }

        ControlFlow::Continue(())
    }
}

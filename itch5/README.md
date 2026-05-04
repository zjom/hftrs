# itch5

A fast, zero-copy parser for the [Nasdaq TotalView-ITCH 5.0](https://www.nasdaqtrader.com/content/technicalsupport/specifications/dataproducts/NQTVITCHspecification.pdf) binary protocol.

ITCH 5.0 is Nasdaq's real-time market data feed. It carries every order book event — adds, cancels, executes, replaces — plus market-wide events, trading halts, and cross-trade notifications, all encoded as compact fixed-length binary messages framed with a 2-byte big-endian length prefix.

## Usage

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
itch5 = "0.1"

# Optional: enable chrono for `messages::Timestamp::to_naive_time` method
# itch5 = {version = "0.1", features = ["chrono"]}  
```

Implement [`MessageHandler`] for the message types you care about, then feed bytes to [`Parser`]:

```rust
use std::ops::ControlFlow;
use itch5::{MessageHandler, Parser, messages::TradeMessage};

#[derive(Default)]
struct TradeCounter {
    count: u64,
}

impl MessageHandler for TradeCounter {
    fn on_trade_message(&mut self, msg: &TradeMessage) -> ControlFlow<()> {
        self.count += 1;
        ControlFlow::Continue(())
    }
}

fn main() {
    let data: &[u8] = /* your ITCH feed bytes */ &[];

    let mut handler = TradeCounter::default();
    Parser::new(data).parse_stream(&mut handler).unwrap();

    println!("trades: {}", handler.count);
}
```

Return `ControlFlow::Break(())` from any handler method to stop parsing early.

## Core API

### `Parser`

```rust
Parser::new(buf: &[u8]) -> Parser<'_>
Parser::parse_stream(&mut self, handler: &mut impl MessageHandler) -> Result<(), ParseError>
```

Iterates over every framed message in `buf`, dispatching each one to the corresponding method on `handler`. The buffer is borrowed for the lifetime of the parser; no heap allocation occurs.

### `parse_one`

```rust
parse_one(buf: &[u8]) -> Result<(&[u8], &[u8]), ParseError>
```

Parses a single length-prefixed message. Returns `(body, remainder)` where `body` starts with the one-byte message-type tag. Useful when you need the raw bytes rather than typed structs — for example, to forward messages to another consumer.

### `MessageHandler`

A visitor trait with one method per message type. Every method has a default no-op implementation, so you only override what you need:

```rust
pub trait MessageHandler {
    fn on_add_order_no_mpid_attribution(&mut self, msg: &AddOrderNoMPIDAttribution) -> ControlFlow<()> { ... }
    fn on_order_executed_message(&mut self, msg: &OrderExecutedMessage) -> ControlFlow<()> { ... }
    fn on_trade_message(&mut self, msg: &TradeMessage) -> ControlFlow<()> { ... }
    // ... 20 more
}
```

### `ParseError`

```rust
pub enum ParseError {
    EmptyBuffer,
    UnknownMessageType(u8),
    MalformedData,
}
```

## Message Types

All 23 ITCH 5.0 message types are covered:

| Tag | Struct | Description |
|-----|--------|-------------|
| `S` | `SystemEventMessage` | Market/session lifecycle events |
| `R` | `StockDirectory` | Daily instrument reference data |
| `H` | `StockTradingAction` | Trading halt and resume |
| `Y` | `RegSHORestriction` | Regulation SHO short-sale restriction |
| `L` | `MarketParticipantPosition` | Market maker status and mode |
| `V` | `MWCBDeclineLevelMessage` | Market-wide circuit breaker levels |
| `W` | `MWCBStatusMessage` | Market-wide circuit breaker status |
| `K` | `QuotingPeriodUpdate` | IPO quotation release |
| `J` | `LULDAuctionCollar` | Limit Up/Limit Down auction collar |
| `h` | `OperationalHalt` | Exchange-specific operational halt |
| `A` | `AddOrderNoMPIDAttribution` | New order (anonymous) |
| `F` | `AddOrderWithMPIDAttribution` | New order (attributed) |
| `E` | `OrderExecutedMessage` | Order execution |
| `C` | `OrderExecutedWithPriceMessage` | Order execution at non-display price |
| `X` | `OrderCancelMessage` | Partial order cancellation |
| `D` | `OrderDeleteMessage` | Full order removal |
| `U` | `OrderReplaceMessage` | Cancel-and-replace |
| `P` | `TradeMessage` | Non-displayed trade |
| `Q` | `CrossTradeMessage` | Opening/closing/halt cross execution |
| `B` | `BrokenTradeMessage` | Trade break |
| `I` | `NetOrderImbalanceIndicatorMessage` | Cross imbalance data |
| `N` | `RetailPriceImprovementIndicator` | Retail interest flags |
| `O` | `DirectListingwithCapitalRaisePriceDiscoveryMessage` | Direct listing price discovery |


## Price Types

Prices in the ITCH protocol are fixed-point integers. The crate provides two newtype wrappers:

| Type   | Bytes | Precision        |
|--------|-------|------------------|
| `Price4` | 4     | 4 decimal places |
| `Price8` | 8     | 8 decimal places |


## Symbol

`Symbol` is an 8-byte ASCII stock ticker, space-padded on the right per the ITCH specification.

```rust
let sym: Symbol = msg.stock();
println!("{}", sym.as_str()); // e.g. "AAPL    " trimmed to "AAPL"

// Parse from a string
let sym: Symbol = "MSFT".parse().unwrap();

// Compact hash/unhash for use as a map key
let key: u64 = sym.hash();
let restored: Symbol = Symbol::from_hash(key);
```


## Timestamp

`Timestamp` is a `u48` representing nanoseconds since midnight.
The crate provides a `Timestamp` newtype wrapper.

```rust
let ts: &Timestamp = Timestamp::ref_from_bytes(b"000001").unwrap();

let dur: std::time::Duration = ts.to_dur();
// requires feature `chrono` to be enabled.
let naivetime: chrono::NaiveTime = ts.to_naive_time();
```

## Wire Format

Each ITCH message is framed as:

```
[2 bytes: big-endian message length][message body]
```

The message body begins with a single ASCII tag byte that identifies the message type, followed by fixed-width fields in network byte order (big-endian). All message structs are `#[repr(C, packed)]` and implement `zerocopy::FromBytes`, so parsing is a bounds-checked pointer cast with no copying.

## Examples

Count every trade in an ITCH binary file:

```
cargo run --example parse_file -- /path/to/feed.itch
```

The source is in [`examples/parse_file.rs`](examples/parse_file.rs). It memory-maps the file and counts `TradeMessage` events.

## Benchmarks

```
ITCH5_BENCH_1M_FILE=/path/to/1M-message.itch cargo bench
```

The benchmark (in [`benches/itch_bench.rs`](benches/itch_bench.rs)) reports throughput in messages/second and bytes/second using [Criterion](https://github.com/bheisler/criterion.rs).

## License

[Unlicense](https://unlicense.org/) — public domain.

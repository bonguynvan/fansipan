//! Properties of the journal's records: whatever an order holds,
//! its record written as a line and read back is the same order, decimals
//! to the last digit and scale. The generators aim at the edges: zero,
//! trailing zeros, and a decimal's full 28 digits of scale.

use execution_engine::OrderRecord;
use fansipan_journal::Record;
use model::{
    ClientOrderId, Decimal, FillId, InstrumentId, Order, OrderKind, OrderStatus, OrderType, Side,
    TimeInForce, VenueOrderId,
};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

const SEED: u64 = 20_261_019;

/// A fixed seed, so every run explores the same cases (tests are
/// deterministic). Set PROPTEST_RNG_SEED to explore others.
fn config() -> ProptestConfig {
    let config = ProptestConfig::default();
    if std::env::var_os("PROPTEST_RNG_SEED").is_some() {
        return config;
    }
    ProptestConfig {
        rng_seed: RngSeed::Fixed(SEED),
        ..config
    }
}

fn decimal() -> impl Strategy<Value = Decimal> {
    prop_oneof![
        Just(Decimal::ZERO),
        Just(Decimal::new(0, 4)),
        Just(Decimal::new(25_000, 3)),
        Just(Decimal::MAX),
        (any::<i64>(), 0u32..=28).prop_map(|(mantissa, scale)| Decimal::new(mantissa, scale)),
    ]
}

fn order_record() -> impl Strategy<Value = OrderRecord> {
    let status = prop::sample::select(vec![
        OrderStatus::Submitted,
        OrderStatus::Accepted,
        OrderStatus::PartiallyFilled,
        OrderStatus::Filled,
        OrderStatus::PendingCancel,
        OrderStatus::PendingAmend,
        OrderStatus::Cancelled,
        OrderStatus::Rejected,
        OrderStatus::Denied,
    ]);
    let kind = prop::sample::select(vec![
        OrderKind::Limit,
        OrderKind::Market,
        OrderKind::MarketToLimit,
    ]);
    let tif = prop::sample::select(vec![
        TimeInForce::Day,
        TimeInForce::ImmediateOrCancel,
        TimeInForce::FillOrKill,
        TimeInForce::AtOpen,
        TimeInForce::AtClose,
        TimeInForce::AfterClose,
    ]);
    (
        (
            any::<u64>(),
            prop::option::of("[A-Z0-9-]{1,12}"),
            "[A-Z]{1,6}",
            any::<bool>(),
        ),
        (decimal(), kind, tif, prop::option::of(decimal())),
        (
            status,
            decimal(),
            prop::option::of(decimal()),
            prop::option::of(".{0,40}"),
        ),
        (
            prop::collection::vec("[a-z0-9:-]{1,20}", 0..4),
            prop::option::of(decimal()),
            any::<bool>(),
        ),
    )
        .prop_map(
            |(
                (id, venue_id, symbol, buy),
                (quantity, kind, tif, limit_price),
                (status, filled, avg, reason),
                (fills, amending_to, external),
            )| OrderRecord {
                order: Order {
                    id: ClientOrderId(id),
                    venue_order_id: venue_id.map(VenueOrderId),
                    instrument: InstrumentId::new("sim", symbol.as_str()),
                    side: if buy { Side::Buy } else { Side::Sell },
                    quantity,
                    order_type: OrderType::new(kind, tif),
                    limit_price,
                    status,
                    filled_quantity: filled,
                    avg_fill_price: avg,
                    reason,
                },
                applied_fills: fills.into_iter().map(FillId).collect(),
                amending_to,
                external,
            },
        )
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn an_order_written_as_a_line_reads_back_the_same(original in order_record()) {
        let line = serde_json::to_string(&Record::order(&original).expect("every shape records")).expect("a line");
        let Record::Order(read) = serde_json::from_str::<Record>(&line).expect("it parses") else {
            panic!("an order reads back as an order");
        };
        let back = read.to_record().expect("it converts");

        prop_assert_eq!(&back, &original);
        // Equal decimals can differ in scale (1.0 and 1.00); the journal keeps it.
        prop_assert_eq!(back.order.quantity.scale(), original.order.quantity.scale());
    }
}

use rust_decimal::Decimal;

use crate::InstrumentId;

/// A net position. Positive quantity is long, negative is short.
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub instrument: InstrumentId,
    pub quantity: Decimal,
    pub avg_price: Decimal,
}

/// Balances and positions as the venue reports them. The venue is the
/// authority here, not core: brokers apply fees, corporate actions, and
/// settlement that core cannot see.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AccountState {
    pub cash: Decimal,
    pub positions: Vec<Position>,
}

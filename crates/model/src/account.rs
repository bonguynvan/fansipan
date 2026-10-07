use rust_decimal::Decimal;

use crate::InstrumentId;

/// A net position. Positive quantity is long, negative is short.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Position {
    pub instrument: InstrumentId,
    pub quantity: Decimal,
    pub avg_price: Decimal,
    /// The part of a long position that has settled, which a market without
    /// short selling lets you sell, before any working sell orders take
    /// their share. In Vietnam, shares bought arrive two trading days later
    /// (T+2). `None` when the venue does not say, which counts as all of it.
    pub settled: Option<Decimal>,
}

impl Position {
    pub fn new(instrument: InstrumentId, quantity: Decimal, avg_price: Decimal) -> Self {
        Self {
            instrument,
            quantity,
            avg_price,
            settled: None,
        }
    }

    pub fn with_settled(self, settled: Decimal) -> Self {
        Self {
            settled: Some(settled),
            ..self
        }
    }

    /// The shares a sell may take before working sell orders: the settled
    /// part of a long position, and nothing of a flat or short one.
    pub fn sellable(&self) -> Decimal {
        self.settled
            .unwrap_or(self.quantity)
            .min(self.quantity)
            .max(Decimal::ZERO)
    }
}

/// Balances and positions as the venue reports them. The venue is the
/// authority here, not core: brokers apply fees, corporate actions, and
/// settlement that core cannot see.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct AccountState {
    pub cash: Decimal,
    pub positions: Vec<Position>,
}

impl AccountState {
    pub fn new(cash: Decimal, positions: Vec<Position>) -> Self {
        Self { cash, positions }
    }

    /// The position in `instrument`, if there is one.
    pub fn position(&self, instrument: &InstrumentId) -> Option<&Position> {
        self.positions
            .iter()
            .find(|position| &position.instrument == instrument)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    fn long(quantity: &str) -> Position {
        Position::new(InstrumentId::new("sim", "VNM"), d(quantity), d("62500"))
    }

    #[test]
    fn only_settled_shares_of_a_long_position_can_be_sold() {
        assert_eq!(long("300").sellable(), d("300"));
        assert_eq!(long("300").with_settled(d("100")).sellable(), d("100"));
        assert_eq!(long("300").with_settled(d("500")).sellable(), d("300"));
        assert_eq!(long("-100").sellable(), Decimal::ZERO);
    }

    #[test]
    fn an_account_finds_a_position_by_instrument() {
        let account = AccountState::new(d("1000"), vec![long("300")]);

        assert!(account.position(&InstrumentId::new("sim", "VNM")).is_some());
        assert!(account.position(&InstrumentId::new("sim", "FPT")).is_none());
    }
}

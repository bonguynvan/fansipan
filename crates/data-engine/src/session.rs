//! What the tape says about an instrument today: last trade, traded volume,
//! high and low. Built from `Trade` prints, so it works for any venue that
//! publishes trades.

use model::Decimal;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionStats {
    pub last: Option<Decimal>,
    pub last_quantity: Option<Decimal>,
    pub volume: Decimal,
    pub high: Option<Decimal>,
    pub low: Option<Decimal>,
    pub trades: u64,
}

impl SessionStats {
    /// The stats with one more trade folded in. A volume past what a
    /// `Decimal` holds stays where it was rather than panicking.
    pub fn with_trade(&self, price: Decimal, quantity: Decimal) -> Self {
        Self {
            last: Some(price),
            last_quantity: Some(quantity),
            volume: self.volume.checked_add(quantity).unwrap_or(self.volume),
            high: Some(self.high.map_or(price, |high| high.max(price))),
            low: Some(self.low.map_or(price, |low| low.min(price))),
            trades: self.trades.saturating_add(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    #[test]
    fn trades_fold_into_last_volume_high_and_low() {
        let stats = SessionStats::default()
            .with_trade(d("62500"), d("100"))
            .with_trade(d("62800"), d("300"))
            .with_trade(d("62400"), d("200"));

        assert_eq!(stats.last, Some(d("62400")));
        assert_eq!(stats.last_quantity, Some(d("200")));
        assert_eq!(stats.volume, d("600"));
        assert_eq!(stats.high, Some(d("62800")));
        assert_eq!(stats.low, Some(d("62400")));
        assert_eq!(stats.trades, 3);
    }

    #[test]
    fn a_quiet_session_has_no_prices() {
        let stats = SessionStats::default();

        assert!(stats.last.is_none() && stats.high.is_none());
        assert_eq!(stats.volume, Decimal::ZERO);
    }
}

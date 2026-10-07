//! Simulated markets: which instruments the sandbox lists and the rules each
//! trades under. The rules are data on every [`Listing`], the same shape a
//! real adapter publishes for its venue; nothing in core knows a market by
//! name. Prices are made up and only roughly in range for each symbol.

use model::{Decimal, InstrumentId, InstrumentInfo, PriceBand, TickTable, UnixNanos};

use crate::{Listing, SimVenue};

/// A market to simulate: listings plus the account and risk defaults that
/// make sense at its price level.
#[derive(Debug, Clone, PartialEq)]
pub struct MarketProfile {
    pub id: &'static str,
    pub title: &'static str,
    pub currency: &'static str,
    pub starting_cash: Decimal,
    pub max_order_quantity: Decimal,
    pub max_order_notional: Decimal,
    pub listings: Vec<Listing>,
}

/// The ids [`MarketProfile::by_id`] knows.
pub const PROFILE_IDS: [&str; 2] = ["vn", "us"];

impl MarketProfile {
    /// The profile for `id` (see [`PROFILE_IDS`]), listed on `venue`.
    pub fn by_id(id: &str, venue: &str) -> Result<Self, String> {
        match id {
            "vn" => vietnam(venue),
            "us" => united_states(venue),
            other => Err(format!(
                "unknown sandbox market \"{other}\"; known: {}",
                PROFILE_IDS.join(", ")
            )),
        }
    }

    /// A venue with every listing of this profile.
    pub fn venue(&self) -> SimVenue {
        let venue = SimVenue::new(self.starting_cash);
        for listing in &self.listings {
            venue.list_with(listing.clone());
        }
        venue
    }
}

/// One exchange's rules: how prices step and how far they may move in a day.
struct Board {
    code: &'static str,
    ticks: TickTable,
    /// Daily limit as a fraction of the reference price; `None` for none.
    band: Option<Decimal>,
    lot: Decimal,
}

fn dec(units: i64, scale: u32) -> Decimal {
    Decimal::new(units, scale)
}

fn listing(
    venue: &str,
    board: &Board,
    currency: &str,
    display_size: Decimal,
    (symbol, name, reference): (&str, &str, Decimal),
) -> Result<Listing, String> {
    let id = InstrumentId::new(venue, symbol);
    let info = InstrumentInfo::new(id.clone(), board.lot, board.ticks.clone())
        .with_name(name)
        .with_exchange(board.code)
        .with_currency(currency);
    let band = match board.band {
        Some(fraction) => {
            PriceBand::from_fraction(id, reference, fraction, &board.ticks, UnixNanos::default())
                .map_err(|e| format!("{symbol}: {e}"))?
        }
        None => PriceBand::new(id, reference, None, None, UnixNanos::default()),
    };
    Ok(Listing {
        info,
        band,
        display_size,
    })
}

/// Vietnam's three boards. HOSE's tick widens with price (10 VND below
/// 10,000, 50 below 50,000, then 100); HNX and UPCoM tick at 100. Daily
/// limits are 7%, 10% and 15% of the reference price. Shares trade in lots
/// of 100.
pub fn vietnam(venue: &str) -> Result<MarketProfile, String> {
    let lot = Decimal::ONE_HUNDRED;
    let flat_100 = TickTable::flat(Decimal::ONE_HUNDRED).map_err(|e| e.to_string())?;
    let hose = Board {
        code: "HOSE",
        ticks: TickTable::new(vec![
            (Decimal::ZERO, dec(10, 0)),
            (dec(10_000, 0), dec(50, 0)),
            (dec(50_000, 0), dec(100, 0)),
        ])
        .map_err(|e| e.to_string())?,
        band: Some(dec(7, 2)),
        lot,
    };
    let hnx = Board {
        code: "HNX",
        ticks: flat_100.clone(),
        band: Some(dec(10, 2)),
        lot,
    };
    let upcom = Board {
        code: "UPCOM",
        ticks: flat_100,
        band: Some(dec(15, 2)),
        lot,
    };
    let display = dec(10_000, 0);
    let on = |board: &Board, entries: &[(&str, &str, i64)]| -> Result<Vec<Listing>, String> {
        entries
            .iter()
            .map(|(symbol, name, price)| {
                listing(venue, board, "VND", display, (symbol, name, dec(*price, 0)))
            })
            .collect()
    };
    let listings = [
        on(
            &hose,
            &[
                ("ACB", "Asia Commercial Bank", 24_500),
                ("FPT", "FPT Corporation", 118_500),
                ("HPG", "Hoa Phat Group", 26_850),
                ("MBB", "Military Commercial Bank", 23_100),
                ("MWG", "Mobile World Investment", 61_200),
                ("SSI", "SSI Securities", 32_400),
                ("TCB", "Techcombank", 25_050),
                ("VCB", "Vietcombank", 61_800),
                ("VIC", "Vingroup", 44_300),
                ("VNM", "Vinamilk", 62_500),
            ],
        )?,
        on(
            &hnx,
            &[
                ("CEO", "C.E.O Group", 21_400),
                ("PVS", "PetroVietnam Technical Services", 33_100),
                ("SHS", "Saigon-Hanoi Securities", 13_600),
            ],
        )?,
        on(
            &upcom,
            &[
                ("ACV", "Airports Corporation of Vietnam", 96_500),
                ("BSR", "Binh Son Refining and Petrochemical", 22_800),
                ("VEA", "VEAM Corporation", 41_200),
            ],
        )?,
    ]
    .concat();

    Ok(MarketProfile {
        id: "vn",
        title: "Vietnam (HOSE, HNX, UPCoM), simulated",
        currency: "VND",
        starting_cash: dec(2_000_000_000, 0),
        max_order_quantity: dec(500_000, 0),
        max_order_notional: dec(20_000_000_000, 0),
        listings,
    })
}

/// A US-style market: a flat one-cent tick, no daily limits, single shares.
pub fn united_states(venue: &str) -> Result<MarketProfile, String> {
    let board = Board {
        code: "NASDAQ",
        ticks: TickTable::flat(dec(1, 2)).map_err(|e| e.to_string())?,
        band: None,
        lot: Decimal::ONE,
    };
    let listings = [
        ("AAPL", "Apple", 19_000),
        ("AMZN", "Amazon", 18_500),
        ("MSFT", "Microsoft", 41_000),
        ("NVDA", "NVIDIA", 88_000),
        ("TSLA", "Tesla", 24_000),
    ]
    .iter()
    .map(|(symbol, name, cents)| {
        listing(
            venue,
            &board,
            "USD",
            Decimal::ONE_HUNDRED,
            (symbol, name, dec(*cents, 2)),
        )
    })
    .collect::<Result<_, _>>()?;

    Ok(MarketProfile {
        id: "us",
        title: "United States, simulated",
        currency: "USD",
        starting_cash: dec(100_000, 0),
        max_order_quantity: dec(1_000, 0),
        max_order_notional: dec(250_000, 0),
        listings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(raw: &str) -> Decimal {
        raw.parse().expect("test decimal")
    }

    #[test]
    fn every_profile_lists_valid_prices_inside_their_bands() {
        for id in PROFILE_IDS {
            let profile = MarketProfile::by_id(id, "sim").unwrap();

            assert!(!profile.listings.is_empty(), "{id}");
            for listing in &profile.listings {
                let reference = listing.band.reference;
                assert!(
                    listing.info.ticks.is_on_tick(reference),
                    "{id} {}",
                    listing.info.id
                );
                assert!(listing.band.contains(reference), "{id} {}", listing.info.id);
                assert_eq!(listing.info.currency, profile.currency);
            }
        }
    }

    #[test]
    fn vietnam_applies_each_boards_rules() {
        let profile = vietnam("sim").unwrap();
        let find = |symbol: &str| {
            profile
                .listings
                .iter()
                .find(|l| l.info.id.symbol == symbol)
                .unwrap()
        };

        let vnm = find("VNM");
        assert_eq!(vnm.info.exchange, "HOSE");
        assert_eq!(vnm.info.lot_size, d("100"));
        assert_eq!(vnm.band.ceiling, Some(d("66800")));
        assert_eq!(vnm.band.floor, Some(d("58200")));

        let shs = find("SHS");
        assert_eq!(shs.info.exchange, "HNX");
        assert_eq!(shs.band.ceiling, Some(d("14900")));
        assert_eq!(shs.band.floor, Some(d("12300")));

        let acv = find("ACV");
        assert_eq!(acv.info.exchange, "UPCOM");
        assert_eq!(acv.band.ceiling, Some(d("110900")));
    }

    #[test]
    fn an_unknown_profile_names_the_known_ones() {
        let err = MarketProfile::by_id("mars", "sim").unwrap_err();

        assert!(err.contains("vn, us"));
    }
}

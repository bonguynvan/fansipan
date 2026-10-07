//! Simulated markets: which instruments the sandbox lists, the rules each
//! trades under, and each exchange's trading day. The rules are data on
//! every [`Listing`] and [`MarketDay`], the same shape a real adapter
//! publishes for its venue; nothing in core knows a market by name. Prices
//! are made up and only roughly in range for each symbol.

use model::{
    Decimal, InstrumentId, InstrumentInfo, NamedOrderType, OrderKind, OrderType, PriceBand,
    SessionPhase, TickTable, TimeInForce, UnixNanos,
};

use crate::{Clock, Listing, MarketDay, Phase, SimVenue, TradingDay};

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
    /// Each exchange's trading day. An exchange without one trades
    /// continuously and publishes no session.
    pub markets: Vec<MarketDay>,
    /// The markets' offset from UTC, in minutes, for real-hours clocks.
    pub utc_offset_minutes: i32,
    /// Trading days until shares bought settle (T+2 in Vietnam).
    pub settlement_days: u64,
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

    /// A venue with every listing of this profile, trading continuously.
    pub fn venue(&self) -> SimVenue {
        let venue = SimVenue::new(self.starting_cash);
        for listing in &self.listings {
            venue.list_with(listing.clone());
        }
        venue
    }

    /// A venue with every listing of this profile, whose exchanges keep
    /// their trading days on `clock`.
    pub fn venue_on(&self, clock: Clock) -> SimVenue {
        self.venue().with_day(TradingDay {
            clock,
            markets: self.markets.clone(),
            settlement_days: self.settlement_days,
        })
    }

    /// Real hours in this profile's markets' time zone.
    pub fn real_clock(&self) -> Clock {
        Clock::Real {
            utc_offset_minutes: self.utc_offset_minutes,
        }
    }
}

/// One exchange's rules: how prices step and how far they may move in a day.
struct Board {
    code: &'static str,
    ticks: TickTable,
    /// Daily limit as a fraction of the reference price; `None` for none.
    band: Option<Decimal>,
    lot: Decimal,
    /// Whether a sell may take more than the settled holding.
    short_selling: bool,
    /// The order types an odd lot may be; none for no odd lots.
    odd_lot_types: &'static [OrderType],
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
        .with_currency(currency)
        .with_short_selling(board.short_selling)
        .with_odd_lot_types(board.odd_lot_types.iter().copied());
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

fn named(kind: OrderKind, time_in_force: TimeInForce, name: &str) -> NamedOrderType {
    NamedOrderType::new(OrderType::new(kind, time_in_force), name)
}

fn phase(
    (hour, minute): (u32, u32),
    phase: SessionPhase,
    name: Option<&str>,
    order_types: &[&NamedOrderType],
) -> Phase {
    Phase {
        start: hour * 60 + minute,
        phase,
        name: name.map(str::to_string),
        order_types: order_types.iter().map(|t| (*t).clone()).collect(),
    }
}

/// Vietnam's trading days, as the exchanges published them after HOSE moved
/// to the KRX system in May 2025:
/// - HOSE: an ATO auction at 9:00 (ATO, LO); continuous trading from 9:15
///   (LO, MTL, which replaced MP), a lunch break 11:30-13:00; an ATC auction
///   at 14:30 (ATC, LO); then negotiated deals only, until 15:00.
/// - HNX: no opening auction; continuous trading from 9:00 (LO, MTL, MOK,
///   MAK); the same break; an ATC auction at 14:30 (ATC, LO); a PLO session
///   at the closing price, 14:45-15:00.
/// - UPCoM: continuous trading 9:00-15:00 (LO only), with the break.
///
/// Orders entered during the break wait for the afternoon. Negotiated deals
/// are not simulated.
fn vietnam_days() -> Vec<MarketDay> {
    use OrderKind::{Limit, Market, MarketToLimit};
    use SessionPhase::{AfterClose, Break, ClosingAuction, Continuous, OpeningAuction};
    let lo = named(Limit, TimeInForce::Day, "LO");
    let mtl = named(MarketToLimit, TimeInForce::Day, "MTL");
    let mok = named(Market, TimeInForce::FillOrKill, "MOK");
    let mak = named(Market, TimeInForce::ImmediateOrCancel, "MAK");
    let ato = named(Market, TimeInForce::AtOpen, "ATO");
    let atc = named(Market, TimeInForce::AtClose, "ATC");
    let plo = named(Market, TimeInForce::AfterClose, "PLO");
    let closed = |at| phase(at, SessionPhase::Closed, None, &[]);
    let lunch = phase((11, 30), Break, None, &[&lo]);
    let day = |market: &str, phases: Vec<Phase>| MarketDay {
        market: market.to_string(),
        phases,
    };
    vec![
        day(
            "HOSE",
            vec![
                closed((0, 0)),
                phase((9, 0), OpeningAuction, Some("ATO"), &[&ato, &lo]),
                phase((9, 15), Continuous, None, &[&lo, &mtl]),
                lunch.clone(),
                phase((13, 0), Continuous, None, &[&lo, &mtl]),
                phase((14, 30), ClosingAuction, Some("ATC"), &[&atc, &lo]),
                phase((14, 45), AfterClose, None, &[]),
                closed((15, 0)),
            ],
        ),
        day(
            "HNX",
            vec![
                closed((0, 0)),
                phase((9, 0), Continuous, None, &[&lo, &mtl, &mok, &mak]),
                lunch.clone(),
                phase((13, 0), Continuous, None, &[&lo, &mtl, &mok, &mak]),
                phase((14, 30), ClosingAuction, Some("ATC"), &[&atc, &lo]),
                phase((14, 45), AfterClose, Some("PLO"), &[&plo]),
                closed((15, 0)),
            ],
        ),
        day(
            "UPCOM",
            vec![
                closed((0, 0)),
                phase((9, 0), Continuous, None, &[&lo]),
                lunch,
                phase((13, 0), Continuous, None, &[&lo]),
                closed((15, 0)),
            ],
        ),
    ]
}

/// Vietnam's three boards. Odd lots (1 to 99 shares) trade as limit orders
/// only. HOSE's tick widens with price (10 VND below
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
        short_selling: false,
        odd_lot_types: &[OrderType::LIMIT],
    };
    let hnx = Board {
        code: "HNX",
        ticks: flat_100.clone(),
        band: Some(dec(10, 2)),
        lot,
        short_selling: false,
        odd_lot_types: &[OrderType::LIMIT],
    };
    let upcom = Board {
        code: "UPCOM",
        ticks: flat_100,
        band: Some(dec(15, 2)),
        lot,
        short_selling: false,
        odd_lot_types: &[OrderType::LIMIT],
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
        markets: vietnam_days(),
        utc_offset_minutes: 7 * 60,
        settlement_days: 2,
    })
}

/// A US-style market: a flat one-cent tick, no daily limits, single shares.
pub fn united_states(venue: &str) -> Result<MarketProfile, String> {
    let board = Board {
        code: "NASDAQ",
        ticks: TickTable::flat(dec(1, 2)).map_err(|e| e.to_string())?,
        band: None,
        lot: Decimal::ONE,
        short_selling: true,
        odd_lot_types: &[],
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
        markets: Vec::new(),
        utc_offset_minutes: -5 * 60,
        settlement_days: 1,
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

    #[test]
    fn every_vietnamese_exchange_has_a_trading_day() {
        let profile = vietnam("sim").unwrap();

        for listing in &profile.listings {
            assert!(
                profile
                    .markets
                    .iter()
                    .any(|day| day.market == listing.info.exchange),
                "{}",
                listing.info.id
            );
        }
        assert!(united_states("sim").unwrap().markets.is_empty());
    }
}

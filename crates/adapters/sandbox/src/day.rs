//! The simulated trading day: each market's phases, and the clock that says
//! which one is current. The phases and the order types each takes are
//! data a profile gives per market, the same shape a real adapter publishes
//! as `Event::Session`. The clock runs either a compressed demo day, so
//! every phase can be tried at any hour, or real hours in the market's time
//! zone.

use model::{NamedOrderType, SessionPhase, SessionStatus, UnixNanos};

const MINUTES_PER_DAY: u32 = 24 * 60;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;
const NANOS_PER_SECOND: u64 = 1_000_000_000;
/// The Unix epoch fell on a Thursday: day 0 is weekday 3, counting Monday
/// as 0.
const EPOCH_WEEKDAY: i64 = 3;
const SATURDAY: i64 = 5;

/// The demo day: seconds into a 15-minute cycle, and the local time (in
/// minutes after midnight) it stands for. Each trading phase lasts long
/// enough to try its order types; between the points, time runs evenly.
const DEMO_DAY: [(u64, u32); 8] = [
    (0, 9 * 60),
    (90, 9 * 60 + 15),
    (300, 11 * 60 + 30),
    (360, 13 * 60),
    (570, 14 * 60 + 30),
    (660, 14 * 60 + 45),
    (750, 15 * 60),
    (900, MINUTES_PER_DAY + 9 * 60),
];
/// The demo day opens on morning continuous trading, so prices move from
/// the start.
const DEMO_START_SECONDS: u64 = 90;

/// One phase of a market's day, from `start` (minutes after local
/// midnight) until the next phase starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Phase {
    pub start: u32,
    pub phase: SessionPhase,
    /// The venue's name for the phase, e.g. "ATO".
    pub name: Option<String>,
    /// What the market takes during the phase.
    pub order_types: Vec<NamedOrderType>,
}

impl Phase {
    pub fn session(&self, market: &str, now: UnixNanos) -> SessionStatus {
        let session = SessionStatus::new(market, self.phase, now)
            .with_order_types(self.order_types.iter().cloned());
        match &self.name {
            Some(name) => session.with_name(name.clone()),
            None => session,
        }
    }
}

/// One market's day, its phases in order of start. Before the first one
/// starts, the day before's last one still holds.
#[derive(Debug, Clone, PartialEq)]
pub struct MarketDay {
    /// As the market's listings name their exchange.
    pub market: String,
    pub phases: Vec<Phase>,
}

impl MarketDay {
    fn phase_at(&self, minute: u32) -> Option<&Phase> {
        self.phases
            .iter()
            .rev()
            .find(|phase| phase.start <= minute)
            .or_else(|| self.phases.last())
    }
}

/// When the simulated markets are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    /// Real time at a fixed offset from UTC, in minutes. Markets are closed
    /// on Saturdays and Sundays; holidays are not modelled.
    Real { utc_offset_minutes: i32 },
    /// The whole trading day every fifteen minutes from `start`, opening on
    /// morning continuous trading.
    Compressed { start: UnixNanos },
}

impl Clock {
    /// Local minutes after midnight at `now`. A day the markets are closed
    /// reads as midnight, which every day's schedule spends closed.
    fn minute(self, now: UnixNanos) -> u32 {
        match self {
            Self::Real { utc_offset_minutes } => {
                let seconds = i64::try_from(now.0 / NANOS_PER_SECOND).unwrap_or(i64::MAX);
                let local = seconds.saturating_add(i64::from(utc_offset_minutes) * 60);
                let weekday = (local.div_euclid(SECONDS_PER_DAY) + EPOCH_WEEKDAY).rem_euclid(7);
                if weekday >= SATURDAY {
                    return 0;
                }
                u32::try_from(local.rem_euclid(SECONDS_PER_DAY) / 60).unwrap_or(0)
            }
            Self::Compressed { start } => {
                let cycle = DEMO_DAY[DEMO_DAY.len() - 1].0;
                let elapsed = now.0.saturating_sub(start.0) / NANOS_PER_SECOND;
                demo_minute((elapsed + DEMO_START_SECONDS) % cycle)
            }
        }
    }
}

/// The local time a point in the demo cycle stands for.
fn demo_minute(second: u64) -> u32 {
    let segment = DEMO_DAY
        .windows(2)
        .find(|pair| second < pair[1].0)
        .unwrap_or(&DEMO_DAY[DEMO_DAY.len() - 2..]);
    let ((from_s, from_m), (to_s, to_m)) = (segment[0], segment[1]);
    let span = u64::from(to_m - from_m);
    let into = (second.saturating_sub(from_s) * span) / (to_s - from_s).max(1);
    (from_m + u32::try_from(into).unwrap_or(0)) % MINUTES_PER_DAY
}

/// The simulated markets' days and the clock they run on.
#[derive(Debug, Clone, PartialEq)]
pub struct TradingDay {
    pub clock: Clock,
    pub markets: Vec<MarketDay>,
}

impl TradingDay {
    /// The phase `market` is in at `now`; `None` for a market with no
    /// schedule, which trades continuously.
    pub fn phase(&self, market: &str, now: UnixNanos) -> Option<&Phase> {
        let minute = self.clock.minute(now);
        self.markets
            .iter()
            .find(|day| day.market == market)
            .and_then(|day| day.phase_at(minute))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::OrderType;

    const SECOND: u64 = NANOS_PER_SECOND;

    fn phase(start: u32, phase: SessionPhase) -> Phase {
        Phase {
            start,
            phase,
            name: None,
            order_types: Vec::new(),
        }
    }

    fn day(clock: Clock) -> TradingDay {
        TradingDay {
            clock,
            markets: vec![MarketDay {
                market: "HOSE".into(),
                phases: vec![
                    phase(0, SessionPhase::Closed),
                    phase(9 * 60, SessionPhase::OpeningAuction),
                    phase(9 * 60 + 15, SessionPhase::Continuous),
                    phase(15 * 60, SessionPhase::Closed),
                ],
            }],
        }
    }

    fn at(day: &TradingDay, now: UnixNanos) -> SessionPhase {
        day.phase("HOSE", now).map(|p| p.phase).unwrap()
    }

    #[test]
    fn the_demo_day_opens_on_continuous_trading_and_cycles() {
        let start = UnixNanos(1_000 * SECOND);
        let demo = day(Clock::Compressed { start });
        let after = |seconds: u64| UnixNanos(start.0 + seconds * SECOND);

        assert_eq!(at(&demo, start), SessionPhase::Continuous);
        // 810 seconds on, the cycle is back at 09:00, the opening auction.
        assert_eq!(at(&demo, after(810)), SessionPhase::OpeningAuction);
        // 660 seconds on: 15:00, closed until the next morning.
        assert_eq!(at(&demo, after(660)), SessionPhase::Closed);
        assert_eq!(at(&demo, after(900)), SessionPhase::Continuous);
    }

    #[test]
    fn demo_time_runs_evenly_between_its_points() {
        assert_eq!(demo_minute(0), 9 * 60);
        assert_eq!(demo_minute(45), 9 * 60 + 7);
        assert_eq!(demo_minute(90), 9 * 60 + 15);
        // The overnight close runs 15:00 to 09:00 in the last 150 seconds.
        assert_eq!(demo_minute(899), 8 * 60 + 52);
    }

    #[test]
    fn real_hours_follow_the_local_clock_and_close_at_weekends() {
        // 2026-10-07 is a Wednesday; 02:30 UTC is 09:30 in Vietnam (UTC+7).
        let wednesday_0930 = UnixNanos(1_791_340_200 * SECOND);
        let real = day(Clock::Real {
            utc_offset_minutes: 7 * 60,
        });

        assert_eq!(at(&real, wednesday_0930), SessionPhase::Continuous);
        let saturday_0930 = UnixNanos(wednesday_0930.0 + 3 * 86_400 * SECOND);
        assert_eq!(at(&real, saturday_0930), SessionPhase::Closed);
    }

    #[test]
    fn a_market_without_a_schedule_has_no_phase() {
        let demo = day(Clock::Compressed {
            start: UnixNanos(0),
        });

        assert!(demo.phase("NASDAQ", UnixNanos(0)).is_none());
    }

    #[test]
    fn a_phase_reads_as_the_session_a_feed_publishes() {
        let ato = Phase {
            name: Some("ATO".into()),
            order_types: vec![NamedOrderType::new(OrderType::LIMIT, "LO")],
            ..phase(9 * 60, SessionPhase::OpeningAuction)
        };

        let session = ato.session("HOSE", UnixNanos(5));

        assert_eq!(session.market, "HOSE");
        assert_eq!(session.name.as_deref(), Some("ATO"));
        assert!(session.accepts(OrderType::LIMIT).is_some());
        assert_eq!(session.ts_event, UnixNanos(5));
    }
}

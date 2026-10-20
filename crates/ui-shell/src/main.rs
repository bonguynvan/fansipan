//! Fansipan as it ships: the terminal wired to the simulated venue.
//!
//! An adapter repo builds the same thing around its own adapter, in a few
//! lines like these.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::ops::RangeInclusive;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fansipan_shell::{run, run_choosing, Choice, Market, RiskLimits, Venue};
use model::UnixNanos;
use sandbox::{
    profiles::PROFILE_IDS, Clock, MarketProfile, SandboxDataClient, SandboxExecutionClient,
    DEFAULT_BEAT,
};

const VENUE: &str = "sim";
/// Which simulated market to run, skipping the trader's choice; see
/// `sandbox::profiles::PROFILE_IDS`. Unset or empty, the trader chooses.
const MARKET_ENV: &str = "FANSIPAN_SANDBOX_MARKET";
/// `real` runs the market's real trading hours; anything else, or nothing,
/// runs a compressed demo day.
const CLOCK_ENV: &str = "FANSIPAN_SANDBOX_CLOCK";
/// How often the simulated market moves, in milliseconds: 500 by default,
/// a few for a busy feed to measure the terminal under load.
const BEAT_ENV: &str = "FANSIPAN_SANDBOX_BEAT_MS";
const BEAT_MS: RangeInclusive<u64> = 1..=60_000;

/// The simulated market `FANSIPAN_SANDBOX_MARKET` names; `None` when it
/// names none, so the trader chooses.
fn market_profile(chosen: Option<String>) -> Result<Option<MarketProfile>, String> {
    match chosen.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(id) => MarketProfile::by_id(id, VENUE)
            .map(Some)
            .map_err(|e| format!("{MARKET_ENV}: {e}")),
    }
}

/// Every simulated market, by title, so none comes first for being the
/// default.
fn market_choices() -> Vec<Choice> {
    let mut profiles: Vec<MarketProfile> = PROFILE_IDS
        .iter()
        .filter_map(|id| MarketProfile::by_id(id, VENUE).ok())
        .collect();
    profiles.sort_by(|a, b| a.title.cmp(b.title));
    profiles
        .iter()
        .map(|profile| Choice::new(profile.id, profile.title))
        .collect()
}

/// The clock `FANSIPAN_SANDBOX_CLOCK` names: real hours, or by default the
/// compressed demo day, starting now.
fn clock(profile: &MarketProfile, chosen: Option<&str>, now: UnixNanos) -> Clock {
    match chosen.map(str::trim) {
        Some("real") => profile.real_clock(),
        _ => Clock::Compressed { start: now },
    }
}

/// The beat `FANSIPAN_SANDBOX_BEAT_MS` names, or the sandbox's own.
fn beat(chosen: Option<&str>) -> Result<Duration, String> {
    let Some(raw) = chosen else {
        return Ok(DEFAULT_BEAT);
    };
    raw.trim()
        .parse::<u64>()
        .ok()
        .filter(|ms| BEAT_MS.contains(ms))
        .map(Duration::from_millis)
        .ok_or_else(|| {
            format!(
                "{BEAT_ENV}: \"{raw}\" is not a whole number of milliseconds from {} to {}",
                BEAT_MS.start(),
                BEAT_MS.end()
            )
        })
}

fn sandbox(profile: &MarketProfile, clock: Clock, beat: Duration) -> Venue {
    let sim = profile.venue_on(clock);
    Venue::new(
        VENUE,
        Market::new(profile.id, profile.title, profile.currency),
        Box::new(SandboxDataClient::new(sim.clone()).with_beat(beat)),
        Box::new(SandboxExecutionClient::new(sim)),
        RiskLimits {
            max_order_quantity: profile.max_order_quantity,
            max_order_notional: profile.max_order_notional,
        },
    )
    .subscribe(profile.listings.iter().map(|l| l.info.id.clone()))
}

fn now() -> UnixNanos {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    UnixNanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

/// The sandbox on market `id`, its demo day starting now.
fn open(id: &str, clock_choice: Option<&str>, beat: Duration) -> Result<Venue, String> {
    let profile = MarketProfile::by_id(id, VENUE)?;
    let clock = clock(&profile, clock_choice, now());
    Ok(sandbox(&profile, clock, beat))
}

fn main() -> ExitCode {
    let chosen = market_profile(env::var(MARKET_ENV).ok())
        .and_then(|profile| Ok((profile, beat(env::var(BEAT_ENV).ok().as_deref())?)));
    let clock_choice = env::var(CLOCK_ENV).ok();
    match chosen {
        Ok((Some(profile), beat)) => run(sandbox(
            &profile,
            clock(&profile, clock_choice.as_deref(), now()),
            beat,
        )),
        Ok((None, beat)) => run_choosing(market_choices(), move |id| {
            open(id, clock_choice.as_deref(), beat)
        }),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_market_is_the_default_so_the_trader_chooses() {
        assert!(market_profile(None).unwrap().is_none());
        assert!(market_profile(Some("  ".into())).unwrap().is_none());
        assert_eq!(
            market_profile(Some(" vn ".into())).unwrap().unwrap().id,
            "vn"
        );
    }

    #[test]
    fn every_market_is_offered_once_in_order_of_title() {
        let mut profiles: Vec<MarketProfile> = PROFILE_IDS
            .iter()
            .map(|id| MarketProfile::by_id(id, VENUE).unwrap())
            .collect();
        profiles.sort_by(|a, b| a.title.cmp(b.title));
        let expected: Vec<Choice> = profiles
            .iter()
            .map(|p| Choice::new(p.id, p.title))
            .collect();

        assert_eq!(market_choices(), expected);
        assert_eq!(expected.len(), PROFILE_IDS.len());
    }

    #[test]
    fn each_offered_market_opens() {
        for choice in PROFILE_IDS {
            assert!(open(choice, None, DEFAULT_BEAT).is_ok(), "{choice}");
        }
        assert!(open("mars", None, DEFAULT_BEAT).is_err());
    }

    #[test]
    fn an_unknown_market_is_refused_naming_the_setting() {
        let err = market_profile(Some("mars".into())).unwrap_err();

        assert!(err.starts_with("FANSIPAN_SANDBOX_MARKET"));
    }

    #[test]
    fn the_sandbox_subscribes_to_every_listing() {
        let profile = market_profile(Some("us".into())).unwrap().unwrap();

        assert_eq!(
            sandbox(&profile, Clock::Compressed { start: now() }, DEFAULT_BEAT)
                .instruments()
                .len(),
            profile.listings.len()
        );
    }

    #[test]
    fn the_beat_is_the_sandboxs_own_unless_set_to_whole_milliseconds() {
        assert_eq!(beat(None), Ok(DEFAULT_BEAT));
        assert_eq!(beat(Some(" 10 ")), Ok(Duration::from_millis(10)));
        for wrong in ["0", "60001", "fast", "1.5", "-3"] {
            assert!(
                beat(Some(wrong)).unwrap_err().starts_with(BEAT_ENV),
                "{wrong}"
            );
        }
    }

    #[test]
    fn the_demo_day_is_the_default_clock() {
        let vn = market_profile(Some("vn".into())).unwrap().unwrap();
        let start = UnixNanos(7);

        assert_eq!(clock(&vn, None, start), Clock::Compressed { start });
        assert_eq!(clock(&vn, Some("demo"), start), Clock::Compressed { start });
        assert_eq!(clock(&vn, Some(" real "), start), vn.real_clock());
    }
}

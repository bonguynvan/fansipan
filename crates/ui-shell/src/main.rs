//! Fansipan as it ships: the terminal wired to the simulated venue.
//!
//! An adapter repo builds the same thing around its own adapter, in a few
//! lines like these.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::ops::RangeInclusive;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fansipan_shell::{run, Market, RiskLimits, Venue};
use model::UnixNanos;
use sandbox::{Clock, MarketProfile, SandboxDataClient, SandboxExecutionClient, DEFAULT_BEAT};

const VENUE: &str = "sim";
/// Which simulated market to run; see `sandbox::profiles::PROFILE_IDS`.
const MARKET_ENV: &str = "FANSIPAN_SANDBOX_MARKET";
const DEFAULT_MARKET: &str = "vn";
/// `real` runs the market's real trading hours; anything else, or nothing,
/// runs a compressed demo day.
const CLOCK_ENV: &str = "FANSIPAN_SANDBOX_CLOCK";
/// How often the simulated market moves, in milliseconds: 500 by default,
/// a few for a busy feed to measure the terminal under load.
const BEAT_ENV: &str = "FANSIPAN_SANDBOX_BEAT_MS";
const BEAT_MS: RangeInclusive<u64> = 1..=60_000;

/// The simulated market named by `FANSIPAN_SANDBOX_MARKET`, or the default.
fn market_profile(chosen: Option<String>) -> Result<MarketProfile, String> {
    let id = chosen.unwrap_or_else(|| DEFAULT_MARKET.to_string());
    MarketProfile::by_id(id.trim(), VENUE).map_err(|e| format!("{MARKET_ENV}: {e}"))
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

fn main() -> ExitCode {
    let chosen = market_profile(env::var(MARKET_ENV).ok())
        .and_then(|profile| Ok((profile, beat(env::var(BEAT_ENV).ok().as_deref())?)));
    match chosen {
        Ok((profile, beat)) => {
            let clock = clock(&profile, env::var(CLOCK_ENV).ok().as_deref(), now());
            run(sandbox(&profile, clock, beat))
        }
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
    fn vietnam_is_the_default_market() {
        assert_eq!(market_profile(None).unwrap().id, "vn");
    }

    #[test]
    fn an_unknown_market_is_refused_naming_the_setting() {
        let err = market_profile(Some("mars".into())).unwrap_err();

        assert!(err.starts_with("FANSIPAN_SANDBOX_MARKET"));
    }

    #[test]
    fn the_sandbox_subscribes_to_every_listing() {
        let profile = market_profile(Some("us".into())).unwrap();

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
        let vn = market_profile(None).unwrap();
        let start = UnixNanos(7);

        assert_eq!(clock(&vn, None, start), Clock::Compressed { start });
        assert_eq!(clock(&vn, Some("demo"), start), Clock::Compressed { start });
        assert_eq!(clock(&vn, Some(" real "), start), vn.real_clock());
    }
}

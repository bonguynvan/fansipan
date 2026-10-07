//! Fansipan as it ships: the terminal wired to the simulated venue.
//!
//! An adapter repo builds the same thing around its own adapter, in a few
//! lines like these (ADR 0008).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::process::ExitCode;

use fansipan_shell::{run, Market, RiskLimits, Venue};
use sandbox::{MarketProfile, SandboxDataClient, SandboxExecutionClient};

const VENUE: &str = "sim";
/// Which simulated market to run; see `sandbox::profiles::PROFILE_IDS`.
const MARKET_ENV: &str = "FANSIPAN_SANDBOX_MARKET";
const DEFAULT_MARKET: &str = "vn";

/// The simulated market named by `FANSIPAN_SANDBOX_MARKET`, or the default.
fn market_profile(chosen: Option<String>) -> Result<MarketProfile, String> {
    let id = chosen.unwrap_or_else(|| DEFAULT_MARKET.to_string());
    MarketProfile::by_id(id.trim(), VENUE).map_err(|e| format!("{MARKET_ENV}: {e}"))
}

fn sandbox(profile: &MarketProfile) -> Venue {
    let sim = profile.venue();
    Venue::new(
        VENUE,
        Market::new(profile.id, profile.title, profile.currency),
        Box::new(SandboxDataClient::new(sim.clone())),
        Box::new(SandboxExecutionClient::new(sim)),
        RiskLimits {
            max_order_quantity: profile.max_order_quantity,
            max_order_notional: profile.max_order_notional,
        },
    )
    .subscribe(profile.listings.iter().map(|l| l.info.id.clone()))
}

fn main() -> ExitCode {
    match market_profile(env::var(MARKET_ENV).ok()) {
        Ok(profile) => run(sandbox(&profile)),
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
            sandbox(&profile).instruments().len(),
            profile.listings.len()
        );
    }
}

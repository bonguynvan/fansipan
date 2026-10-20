//! The quote path, measured as a distribution. A quote
//! an adapter publishes on the bus is routed by the shell's pump into the
//! data engine and the pending batch; this times each one, from when it was
//! due to when it was routed, at a steady 1,000, 10,000, and 100,000 quotes
//! a second across a board of symbols, and reports P50, P99, P99.9, and
//! max. It also times each 50 ms flush that conflates the batch and
//! serializes it for the front end, and counts what a subscriber that
//! stops reading piles up.
//!
//! Timing is no part of the ordinary suite: these run on purpose, in
//! release, and print a table of what they measured:
//!
//! ```text
//! cargo test -p fansipan-shell --release --lib latency -- --ignored --nocapture --test-threads 1
//! ```
//!
//! The budgets asserted are on medians, about a hundred times what a
//! desktop measures: one preemption on a shared CI runner moves a tail but
//! not a median, so they catch a broken path without failing on noise. The
//! tails are in the table, to be read.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use adapter_api::{AdapterError, DataClient, ExecutionClient};
use bus::BusHandle;
use model::{AccountState, CancelOrder, Quote, SubmitOrder, UnixNanos};

use super::*;

const SYMBOLS: usize = 30;
const WARMUP: Duration = Duration::from_millis(300);
const MEASURED: Duration = Duration::from_secs(2);
const P50_BUDGET: Duration = Duration::from_millis(1);
const FLUSH_P50_BUDGET: Duration = Duration::from_millis(5);

/// An adapter that publishes nothing of its own, so the only load on the
/// bus is what the benchmark sends.
struct Quiet;

impl DataClient for Quiet {
    fn connect(&mut self, _bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
        Ok(())
    }
    fn subscribe_quotes(&mut self, _instrument: &InstrumentId) -> Result<(), AdapterError> {
        Ok(())
    }
    fn unsubscribe_quotes(&mut self, _instrument: &InstrumentId) -> Result<(), AdapterError> {
        Ok(())
    }
    fn subscribe_trades(&mut self, _instrument: &InstrumentId) -> Result<(), AdapterError> {
        Ok(())
    }
    fn unsubscribe_trades(&mut self, _instrument: &InstrumentId) -> Result<(), AdapterError> {
        Ok(())
    }
    fn disconnect(&mut self) -> Result<(), AdapterError> {
        Ok(())
    }
}

impl ExecutionClient for Quiet {
    fn connect(&mut self, _bus: Box<dyn BusHandle>) -> Result<(), AdapterError> {
        Ok(())
    }
    fn submit_order(&mut self, _order: SubmitOrder) -> Result<(), AdapterError> {
        Ok(())
    }
    fn cancel_order(&mut self, _cancel: CancelOrder) -> Result<(), AdapterError> {
        Ok(())
    }
    fn account_state(&mut self) -> Result<AccountState, AdapterError> {
        Ok(AccountState::default())
    }
    fn disconnect(&mut self) -> Result<(), AdapterError> {
        Ok(())
    }
}

fn symbols() -> Vec<InstrumentId> {
    (0..SYMBOLS)
        .map(|i| InstrumentId::new("sim", format!("S{i:02}")))
        .collect()
}

fn shell(bus: &Bus) -> Shell {
    let venue = Venue::new(
        "sim",
        Market::new("bench", "Latency bench", "VND"),
        Box::new(Quiet),
        Box::new(Quiet),
        RiskLimits {
            max_order_quantity: Decimal::from(1_000),
            max_order_notional: Decimal::from(1_000_000),
        },
    )
    .subscribe(symbols());
    connect_engines(bus, venue).expect("the shell connects")
}

/// One run's measurements, each list sorted.
struct Run {
    rate: u64,
    sent: u64,
    /// Due to routed, nanoseconds, for quotes after the warm-up.
    latencies: Vec<u64>,
    /// Each flush's take-and-serialize after the warm-up, nanoseconds.
    flushes: Vec<u64>,
    /// Quotes the shell's routing refused, and flushes that could not be
    /// taken or serialized.
    failed: usize,
    /// What a subscriber that never read held at the end.
    backlog: usize,
}

fn nanos(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

/// Publishes quotes at `rate` a second for the warm-up and the measured
/// span, through the shell's own routing and flushing. Each quote is
/// stamped with the time it was due, not the time it went out, so a
/// publisher that stalls and catches up shows as latency instead of
/// hiding it.
fn run(rate: u64) -> Run {
    let bus = Bus::new();
    let pumped = bus.subscribe();
    let stalled = bus.subscribe();
    let shell = Arc::new(shell(&bus));
    let start = Instant::now();
    let warmup = u64::try_from(WARMUP.as_nanos()).unwrap_or(u64::MAX);
    let done = Arc::new(AtomicBool::new(false));

    let pump = {
        let shell = Arc::clone(&shell);
        thread::spawn(move || {
            let mut latencies = Vec::with_capacity(usize::try_from(rate * 3).unwrap_or(0));
            let mut failed = 0;
            for event in pumped {
                let due = match &event {
                    Event::Quote(quote) => Some(quote.ts_event.0),
                    _ => None,
                };
                if route(&shell, &event).is_err() {
                    failed += 1;
                    continue;
                }
                if let Some(due) = due.filter(|at| *at >= warmup) {
                    latencies.push(nanos(start).saturating_sub(due));
                }
            }
            (latencies, failed)
        })
    };
    let flusher = {
        let shell = Arc::clone(&shell);
        let done = Arc::clone(&done);
        thread::spawn(move || {
            let mut flushes = Vec::new();
            let mut failed = 0;
            while !done.load(Ordering::Relaxed) {
                thread::sleep(FLUSH_INTERVAL);
                let began = Instant::now();
                let sent = match take_batch(&shell) {
                    Ok(Some(batch)) => serde_json::to_string(&batch).is_ok(),
                    // An empty batch sends nothing, so it is no flush to time.
                    Ok(None) => continue,
                    Err(_) => false,
                };
                if !sent {
                    failed += 1;
                } else if nanos(start) >= warmup {
                    flushes.push(nanos(began));
                }
            }
            (flushes, failed)
        })
    };

    let instruments = symbols();
    let interval = 1_000_000_000 / rate;
    let total = u64::try_from((WARMUP + MEASURED).as_nanos()).unwrap_or(u64::MAX);
    let mut sent = 0u64;
    loop {
        let now = nanos(start);
        if now >= total {
            break;
        }
        let due = sent * interval;
        if now < due {
            // Sleep away most of a long wait; spin the last stretch, which
            // a sleep on Windows would overshoot by a millisecond or more.
            let wait = due - now;
            if wait > 2_000_000 {
                thread::sleep(Duration::from_nanos(wait - 1_000_000));
            } else {
                std::hint::spin_loop();
            }
            continue;
        }
        let step = Decimal::from(sent % 50);
        let instrument = instruments[usize::try_from(sent).unwrap_or(0) % SYMBOLS].clone();
        bus.publish(Event::Quote(Quote {
            instrument,
            bid: Decimal::from(25_000) + step,
            ask: Decimal::from(25_050) + step,
            bid_size: Decimal::from(1_000),
            ask_size: Decimal::from(1_200),
            ts_event: UnixNanos(due),
        }));
        sent += 1;
    }

    done.store(true, Ordering::Relaxed);
    let backlog = stalled.try_iter().count();
    drop(stalled);
    // Dropping the bus ends the pump once it has routed what is queued, and
    // only because `Quiet` keeps none of the bus handles it is given.
    drop(bus);
    let (mut latencies, failed_routes) = pump.join().expect("the pump finishes");
    let (mut flushes, failed_flushes) = flusher.join().expect("the flusher finishes");
    latencies.sort_unstable();
    flushes.sort_unstable();
    Run {
        rate,
        sent,
        latencies,
        flushes,
        failed: failed_routes + failed_flushes,
        backlog,
    }
}

/// The value at quantile `q` of `sorted`: P50 is 0.5, P99.9 is 0.999.
fn quantile(sorted: &[u64], q: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let rank = ((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

fn micros(ns: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let us = ns as f64 / 1_000.0;
    if us < 10.0 {
        format!("{us:.1}")
    } else {
        format!("{us:.0}")
    }
}

/// A report line per rate, as a markdown table row. A
/// flush comes every 50 ms, too few in a run for a P99 that is not just
/// the maximum, so the maximum is what it reports.
fn report(runs: &[Run]) -> String {
    let mut out = String::from(
        "| Quotes/s | Sent | P50 µs | P99 µs | P99.9 µs | Max µs | Flushes | Flush P50 µs | Flush max µs | Stalled backlog |\n\
         |---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for run in runs {
        let l = &run.latencies;
        let f = &run.flushes;
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            run.rate,
            run.sent,
            micros(quantile(l, 0.5)),
            micros(quantile(l, 0.99)),
            micros(quantile(l, 0.999)),
            micros(l.last().copied().unwrap_or(0)),
            f.len(),
            micros(quantile(f, 0.5)),
            micros(f.last().copied().unwrap_or(0)),
            run.backlog,
        ));
    }
    out.push_str(&format!(
        "\nA stalled subscriber holds each event it has not read: {} bytes of `Event` each, plus the instrument id's two strings.\n",
        std::mem::size_of::<Event>()
    ));
    out
}

#[test]
#[ignore = "a timing run: cargo test --release --lib latency -- --ignored --nocapture"]
fn latency_of_the_quote_path() {
    let runs: Vec<Run> = [1_000, 10_000, 100_000].into_iter().map(run).collect();
    let table = report(&runs);
    println!("\n{table}");
    if let Some(path) = std::env::var_os("FANSIPAN_LATENCY_REPORT") {
        std::fs::write(path, &table).expect("the report is written");
    }

    for run in &runs {
        assert_eq!(
            run.failed, 0,
            "{} quotes/s: quotes or flushes failed",
            run.rate
        );
        assert!(
            !run.latencies.is_empty() && !run.flushes.is_empty(),
            "{} quotes/s: nothing was measured",
            run.rate
        );
        let flush = Duration::from_nanos(quantile(&run.flushes, 0.5));
        assert!(
            flush <= FLUSH_P50_BUDGET,
            "{} quotes/s: flush P50 {flush:?}",
            run.rate
        );
        // A slower runner may fall behind at the top rate, and then every
        // quote waits in the queue; it must keep up with the rates below.
        if run.rate <= 10_000 {
            let p50 = Duration::from_nanos(quantile(&run.latencies, 0.5));
            assert!(p50 <= P50_BUDGET, "{} quotes/s: P50 {p50:?}", run.rate);
        }
    }
}

#[test]
fn quantiles_take_the_value_at_that_rank() {
    let sorted: Vec<u64> = (1..=1_000).collect();

    assert_eq!(quantile(&sorted, 0.5), 500);
    assert_eq!(quantile(&sorted, 0.99), 990);
    assert_eq!(quantile(&sorted, 0.999), 999);
    assert_eq!(quantile(&sorted, 1.0), 1_000);
    assert_eq!(quantile(&[], 0.5), 0);
}

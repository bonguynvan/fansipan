//! The engines behind the window: the adapter's data client connected to a
//! data engine through the bus, and a thread that applies every event to the
//! engine and asks the window for a frame. The window reads the engine when
//! it paints, at most once per frame, however fast the feed runs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use adapter_api::DataClient;
use bus::{Bus, Event};
use data_engine::DataEngine;
use densa::window::Waker;
use model::InstrumentId;

/// The data engine the window reads, and whether it changed since the last
/// frame read it.
#[derive(Clone)]
pub struct Feed {
    data: Arc<Mutex<DataEngine>>,
    changed: Arc<AtomicBool>,
}

impl Feed {
    /// Connects `client`, subscribes to `instruments`, and starts the thread
    /// that keeps the engine up to date and wakes the window through `waker`.
    pub fn start(
        client: Box<dyn DataClient>,
        instruments: &[InstrumentId],
        waker: Waker,
    ) -> Result<Self, String> {
        let bus = Bus::new();
        let events = bus.subscribe();
        let mut data = DataEngine::new(client);
        data.connect(bus.handle()).map_err(|e| e.to_string())?;
        for instrument in instruments {
            data.subscribe_quotes(instrument)
                .map_err(|e| e.to_string())?;
            data.subscribe_trades(instrument)
                .map_err(|e| e.to_string())?;
        }
        let feed = Self {
            data: Arc::new(Mutex::new(data)),
            changed: Arc::new(AtomicBool::new(true)),
        };
        let pump = feed.clone();
        thread::Builder::new()
            .name("feed".into())
            .spawn(move || {
                // The bus keeps a sender for as long as the engine holds a
                // handle to it, so this runs for the life of the window.
                let _bus = bus;
                for event in events {
                    let Ok(mut data) = pump.data.lock() else {
                        return;
                    };
                    apply(&mut data, &event);
                    drop(data);
                    if !pump.changed.swap(true, Ordering::AcqRel) && !waker.wake() {
                        return;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(feed)
    }

    /// The engine as it is now; `None` if the feed thread failed while
    /// holding it.
    pub fn engine(&self) -> Option<MutexGuard<'_, DataEngine>> {
        self.data.lock().ok()
    }

    /// The engine, when something changed since the last call; `None`
    /// otherwise, or if the feed thread failed while holding it.
    pub fn take_changes(&self) -> Option<MutexGuard<'_, DataEngine>> {
        if !self.changed.swap(false, Ordering::AcqRel) {
            return None;
        }
        self.data.lock().ok()
    }
}

/// One bus event into the data engine. Orders and the account are the
/// execution engine's, which this window does not show yet.
fn apply(data: &mut DataEngine, event: &Event) {
    match event {
        Event::Quote(quote) => {
            data.on_quote(quote);
        }
        Event::Trade(trade) => {
            data.on_trade(trade);
        }
        Event::Depth(depth) => {
            data.on_depth(depth);
        }
        Event::Instrument(info) => data.on_instrument(info),
        Event::PriceBand(band) => data.on_band(band),
        Event::Session(session) => data.on_session(session),
        Event::InstrumentStatus(status) => data.on_instrument_status(status),
        Event::Connection(status) => {
            data.on_connection(status);
        }
        _ => {}
    }
}

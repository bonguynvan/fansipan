use std::fmt;

/// A venue-qualified instrument identity, e.g. "BINANCE:BTCUSDT".
/// Deliberately opaque to core: adapters decide what the symbol part means.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstrumentId {
    pub venue: String,
    pub symbol: String,
}

impl InstrumentId {
    pub fn new(venue: impl Into<String>, symbol: impl Into<String>) -> Self {
        Self {
            venue: venue.into(),
            symbol: symbol.into(),
        }
    }
}

impl fmt::Display for InstrumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.venue, self.symbol)
    }
}

/// Assigned by the execution engine when an order is created, before any
/// adapter sees it. Every execution report an adapter emits is keyed by it,
/// which is what lets the engine own order state without trusting the venue
/// to name orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientOrderId(pub u64);

impl fmt::Display for ClientOrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The venue's own name for an order, learned from its acceptance. Opaque:
/// venues use integers, UUIDs, or composite strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VenueOrderId(pub String);

/// Identifies one fill of one order. Venues resend fills after a reconnect,
/// so the engine drops a fill whose id it has already applied. An adapter for
/// a venue that sends no fill id must derive a stable one (e.g. from the
/// venue's trade sequence number), never a random one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FillId(pub String);

/// Nanoseconds since the Unix epoch, as stamped by whoever observed the event
/// (the venue when it provides a time, the adapter otherwise). `model` never
/// reads a clock itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct UnixNanos(pub u64);

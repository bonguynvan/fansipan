//! The price board: every listed instrument, three levels a side, the last
//! match, and the day's range, each price coloured against the day's
//! reference, ceiling, and floor. Rows are rebuilt when the engine changed;
//! painting a cell only copies its text.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use data_engine::DataEngine;
use densa::core::Color;
use densa::grid::{Align, CellStyle, CellText, Column, GridModel, Palette};
use model::{BookLevel, Decimal, InstrumentId, PriceBand};

use crate::i18n::Words;

/// How long a changed match price stays lit.
const FLASH: Duration = Duration::from_millis(300);
/// Book levels shown a side.
const LEVELS: usize = 3;
const PERCENT_PLACES: u32 = 2;

/// How a market's prices read on a board: divided by `scale`, at `places`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceLook {
    pub scale: Decimal,
    pub places: u32,
}

impl PriceLook {
    /// The board's preset for a market id; plain prices at two places
    /// otherwise. Vietnamese boards quote in thousands of the currency.
    pub fn of(market: &str) -> Self {
        match market {
            "vn" => Self {
                scale: Decimal::from(1000),
                places: 2,
            },
            _ => Self {
                scale: Decimal::ONE,
                places: 2,
            },
        }
    }
}

/// What colours a price: where it stands against the day's band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Plain,
    Reference,
    Ceiling,
    Floor,
    Up,
    Down,
}

fn tone(price: Option<Decimal>, band: Option<&PriceBand>) -> Tone {
    let (Some(price), Some(band)) = (price, band) else {
        return Tone::Plain;
    };
    if band.ceiling == Some(price) {
        Tone::Ceiling
    } else if band.floor == Some(price) {
        Tone::Floor
    } else if price == band.reference {
        Tone::Reference
    } else if price > band.reference {
        Tone::Up
    } else {
        Tone::Down
    }
}

/// The board's columns, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Symbol,
    Reference,
    Ceiling,
    Floor,
    BidPrice(usize),
    BidSize(usize),
    Last,
    LastSize,
    Change,
    Percent,
    AskPrice(usize),
    AskSize(usize),
    Volume,
    High,
    Low,
}

fn fields() -> Vec<Field> {
    let mut fields = vec![
        Field::Symbol,
        Field::Reference,
        Field::Ceiling,
        Field::Floor,
    ];
    for level in (0..LEVELS).rev() {
        fields.extend([Field::BidPrice(level), Field::BidSize(level)]);
    }
    fields.extend([Field::Last, Field::LastSize, Field::Change, Field::Percent]);
    for level in 0..LEVELS {
        fields.extend([Field::AskPrice(level), Field::AskSize(level)]);
    }
    fields.extend([Field::Volume, Field::High, Field::Low]);
    fields
}

fn column(field: Field, words: &Words) -> Column {
    let n = |level: usize| (level + 1).to_string();
    let (title, chars) = match field {
        Field::Symbol => (words.t("column.symbol", &[]), 6),
        Field::Reference => (words.t("board.ref", &[]), 7),
        Field::Ceiling => (words.t("board.ceiling", &[]), 7),
        Field::Floor => (words.t("board.floor", &[]), 7),
        Field::BidPrice(l) | Field::AskPrice(l) => (words.t("board.priceN", &[("n", &n(l))]), 7),
        Field::BidSize(l) | Field::AskSize(l) => (words.t("board.volumeN", &[("n", &n(l))]), 8),
        Field::Last => (words.t("board.price", &[]), 7),
        Field::LastSize => (words.t("board.volume", &[]), 8),
        Field::Change => (words.t("board.change", &[]), 6),
        Field::Percent => (words.t("board.pct", &[]), 6),
        Field::Volume => (words.t("board.totalVolume", &[]), 11),
        Field::High => (words.t("board.high", &[]), 7),
        Field::Low => (words.t("board.low", &[]), 7),
    };
    let align = if field == Field::Symbol {
        Align::Left
    } else {
        Align::Right
    };
    Column::new(title, chars, align)
}

/// One instrument's row, ready to paint.
#[derive(Debug, Clone)]
struct Row {
    instrument: InstrumentId,
    texts: Vec<String>,
    tones: Vec<Tone>,
    last: Option<Decimal>,
    /// When the match price last moved, and whether up.
    moved: Option<(Instant, bool)>,
}

/// The board as a grid model.
pub struct BoardModel {
    fields: Vec<Field>,
    columns: Vec<Column>,
    rows: Vec<Row>,
    look: PriceLook,
    now: Instant,
}

impl BoardModel {
    pub fn new(words: &Words, look: PriceLook) -> Self {
        let fields = fields();
        let columns = fields.iter().map(|&f| column(f, words)).collect();
        Self {
            fields,
            columns,
            rows: Vec::new(),
            look,
            now: Instant::now(),
        }
    }

    /// Rebuilds every row from the engine, keeping each instrument's last
    /// move so its flash carries on.
    pub fn refresh(&mut self, data: &DataEngine, now: Instant) {
        self.now = now;
        let previous: Vec<Row> = std::mem::take(&mut self.rows);
        self.rows = data
            .instruments()
            .map(|info| {
                let before = previous.iter().find(|r| r.instrument == info.id);
                self.row(data, &info.id, before, now)
            })
            .collect();
    }

    /// Advances the clock the flashes fade by; true while any is lit.
    pub fn tick(&mut self, now: Instant) -> bool {
        self.now = now;
        self.rows.iter().any(|r| {
            r.moved
                .is_some_and(|(at, _)| now.duration_since(at) < FLASH)
        })
    }

    /// The instrument on row `row`, as the grid shows it.
    pub fn instrument(&self, row: usize) -> Option<&InstrumentId> {
        self.rows.get(row).map(|r| &r.instrument)
    }

    fn row(&self, data: &DataEngine, id: &InstrumentId, before: Option<&Row>, now: Instant) -> Row {
        let band = data.band(id);
        let stats = data.stats(id).cloned().unwrap_or_default();
        let (bids, asks) = match (data.depth(id), data.latest(id)) {
            (Some(depth), _) => (depth.bids.clone(), depth.asks.clone()),
            (None, Some(quote)) => (
                vec![BookLevel::new(quote.bid, quote.bid_size)],
                vec![BookLevel::new(quote.ask, quote.ask_size)],
            ),
            (None, None) => (Vec::new(), Vec::new()),
        };
        let reference = band.map(|b| b.reference);
        let change = stats
            .last
            .zip(reference)
            .and_then(|(l, r)| l.checked_sub(r));
        let percent = change.zip(reference).and_then(|(c, r)| {
            c.checked_mul(Decimal::ONE_HUNDRED)
                .and_then(|scaled| scaled.checked_div(r))
                .map(|p| p.round_dp(PERCENT_PLACES))
        });
        let last_tone = tone(stats.last, band);
        let mut texts = Vec::with_capacity(self.fields.len());
        let mut tones = Vec::with_capacity(self.fields.len());
        for &field in &self.fields {
            let (text, colour) = match field {
                Field::Symbol => (id.symbol.clone(), last_tone),
                Field::Reference => (self.price(reference), Tone::Reference),
                Field::Ceiling => (self.price(band.and_then(|b| b.ceiling)), Tone::Ceiling),
                Field::Floor => (self.price(band.and_then(|b| b.floor)), Tone::Floor),
                Field::BidPrice(l) => level_price(self, &bids, l, band),
                Field::BidSize(l) => (
                    size(bids.get(l).map(|b| b.size)),
                    level_tone(&bids, l, band),
                ),
                Field::AskPrice(l) => level_price(self, &asks, l, band),
                Field::AskSize(l) => (
                    size(asks.get(l).map(|b| b.size)),
                    level_tone(&asks, l, band),
                ),
                Field::Last => (self.price(stats.last), last_tone),
                Field::LastSize => (size(stats.last_quantity), last_tone),
                Field::Change => (
                    signed(change.map(|c| c / self.look.scale), self.look.places),
                    last_tone,
                ),
                Field::Percent => (signed(percent, PERCENT_PLACES), last_tone),
                Field::Volume => (size(Some(stats.volume)), Tone::Plain),
                Field::High => (self.price(stats.high), tone(stats.high, band)),
                Field::Low => (self.price(stats.low), tone(stats.low, band)),
            };
            texts.push(text);
            tones.push(colour);
        }
        let moved = match (before.and_then(|b| b.last), stats.last) {
            (Some(was), Some(is)) if was != is => Some((now, is > was)),
            _ => before.and_then(|b| b.moved),
        };
        Row {
            instrument: id.clone(),
            texts,
            tones,
            last: stats.last,
            moved,
        }
    }

    fn price(&self, value: Option<Decimal>) -> String {
        value.map_or_else(String::new, |v| {
            format!("{:.*}", self.look.places as usize, v / self.look.scale)
        })
    }
}

fn level_price(
    board: &BoardModel,
    book: &[BookLevel],
    level: usize,
    band: Option<&PriceBand>,
) -> (String, Tone) {
    (
        board.price(book.get(level).map(|b| b.price)),
        level_tone(book, level, band),
    )
}

fn level_tone(book: &[BookLevel], level: usize, band: Option<&PriceBand>) -> Tone {
    tone(book.get(level).map(|b| b.price), band)
}

fn signed(value: Option<Decimal>, places: u32) -> String {
    value.map_or_else(String::new, |v| {
        let sign = if v > Decimal::ZERO { "+" } else { "" };
        format!("{sign}{:.*}", places as usize, v)
    })
}

/// A whole quantity with thousands grouped: 1,234,500.
fn size(value: Option<Decimal>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let digits = value.trunc().abs().to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// The board's price tones, as the web front end colours them.
fn colour(tone: Tone, palette: &Palette) -> Color {
    match tone {
        Tone::Plain => palette.text,
        Tone::Reference => Color::srgb(0xFA_CC15),
        Tone::Ceiling => Color::srgb(0xE8_79F9),
        Tone::Floor => Color::srgb(0x22_D3EE),
        Tone::Up => Color::srgb(0x4A_DE80),
        Tone::Down => Color::srgb(0xF8_7171),
    }
}

impl GridModel for BoardModel {
    fn columns(&self) -> &[Column] {
        &self.columns
    }

    fn rows(&self) -> usize {
        self.rows.len()
    }

    fn cell(&self, row: usize, column: usize, text: &mut CellText, palette: &Palette) -> CellStyle {
        let Some(r) = self.rows.get(row) else {
            return CellStyle::new(palette.text);
        };
        let _ = text.write_str(r.texts.get(column).map_or("", String::as_str));
        let style = CellStyle::new(colour(
            r.tones.get(column).copied().unwrap_or(Tone::Plain),
            palette,
        ));
        let lit = self.fields.get(column) == Some(&Field::Last)
            && r.moved
                .is_some_and(|(at, _)| self.now.duration_since(at) < FLASH);
        if !lit {
            return style;
        }
        let up = r.moved.is_some_and(|(_, up)| up);
        let flash = if up {
            palette.positive
        } else {
            palette.negative
        };
        style.with_background(Some(flash.with_alpha(0.35)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(text: &str) -> Decimal {
        Decimal::from_str(text).unwrap()
    }

    fn band() -> PriceBand {
        PriceBand::new(
            InstrumentId::new("sim", "AAA"),
            d("100"),
            Some(d("107")),
            Some(d("93")),
            model::UnixNanos(0),
        )
    }

    #[test]
    fn a_price_takes_its_tone_from_the_band() {
        let band = band();
        assert_eq!(tone(Some(d("107")), Some(&band)), Tone::Ceiling);
        assert_eq!(tone(Some(d("93")), Some(&band)), Tone::Floor);
        assert_eq!(tone(Some(d("100")), Some(&band)), Tone::Reference);
        assert_eq!(tone(Some(d("101")), Some(&band)), Tone::Up);
        assert_eq!(tone(Some(d("99")), Some(&band)), Tone::Down);
        assert_eq!(tone(None, Some(&band)), Tone::Plain);
        assert_eq!(tone(Some(d("99")), None), Tone::Plain);
    }

    #[test]
    fn sizes_group_thousands_and_prices_follow_the_look() {
        assert_eq!(size(Some(d("1234500"))), "1,234,500");
        assert_eq!(size(Some(d("100"))), "100");
        assert_eq!(size(None), "");
        let board = BoardModel::new(&Words::new("en"), PriceLook::of("vn"));
        assert_eq!(board.price(Some(d("25750"))), "25.75");
        assert_eq!(signed(Some(d("1.4")), 2), "+1.40");
        assert_eq!(signed(Some(d("-0.05")), 2), "-0.05");
    }

    #[test]
    fn the_columns_run_bids_inward_and_asks_outward() {
        let board = BoardModel::new(&Words::new("en"), PriceLook::of("us"));
        let titles: Vec<&str> = board.columns.iter().map(|c| c.title.as_ref()).collect();

        assert_eq!(titles.len(), 23);
        assert_eq!(
            &titles[4..10],
            ["Price 3", "Vol 3", "Price 2", "Vol 2", "Price 1", "Vol 1"]
        );
        assert_eq!(&titles[14..16], ["Price 1", "Vol 1"]);
    }
}

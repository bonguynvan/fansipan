//! The chart of one instrument: the engine's candles with a moving average
//! over them, and the ticks folded into each candle below. It follows the
//! symbol picked on the board.

use data_engine::{Bar, DataEngine};
use densa::chart::{
    candle_extent, candles_within, paint_bars, paint_candles, paint_line, points_within, y_extent,
    Axis, BarStyle, Candle, CandleStyle, ChartModel, ChartPalette, LineStyle, Plot,
};
use densa::draw::Canvas;
use model::{Decimal, InstrumentId};

/// Candles kept from the engine.
const CANDLES: usize = 500;
/// The moving average's period, in candles.
const AVERAGE: usize = 20;
const NANOS_PER_SECOND: f64 = 1e9;

/// Display only: the chart draws in floating point.
fn float(value: Decimal) -> f64 {
    value.to_string().parse().unwrap_or(0.0)
}

#[derive(Default)]
pub struct ChartData {
    /// Prices are shown divided by this, as the board shows them.
    scale: Decimal,
    instrument: Option<InstrumentId>,
    candles: Vec<(f64, Candle)>,
    average: Vec<(f64, f64)>,
    ticks: Vec<(f64, f64)>,
    /// One candle's period, in seconds.
    step: f64,
}

impl ChartData {
    /// A chart showing prices divided by `scale`.
    pub fn new(scale: Decimal) -> Self {
        Self {
            scale,
            ..Self::default()
        }
    }

    pub fn instrument(&self) -> Option<&InstrumentId> {
        self.instrument.as_ref()
    }

    /// Shows `instrument` from now on; its history comes with the next
    /// refresh.
    pub fn follow(&mut self, instrument: InstrumentId) {
        if self.instrument.as_ref() != Some(&instrument) {
            *self = Self {
                instrument: Some(instrument),
                ..Self::new(self.scale)
            };
        }
    }

    /// Takes the followed instrument's candles from the engine.
    pub fn refresh(&mut self, data: &DataEngine) {
        let Some(instrument) = &self.instrument else {
            return;
        };
        let bars = data.bars(instrument, CANDLES);
        let x = |bar: &Bar| bar.ts_open.0 as f64 / NANOS_PER_SECOND;
        self.step = match bars.as_slice() {
            [.., before, last] => (x(last) - x(before)).max(1.0),
            _ => 1.0,
        };
        let scaled = |price: Decimal| float(price.checked_div(self.scale).unwrap_or(price));
        self.candles = bars
            .iter()
            .map(|b| {
                (
                    x(b),
                    Candle::new(
                        scaled(b.open),
                        scaled(b.high),
                        scaled(b.low),
                        scaled(b.close),
                    ),
                )
            })
            .collect();
        self.ticks = bars.iter().map(|b| (x(b), b.ticks as f64)).collect();
        self.average = exponential_average(&self.candles, AVERAGE);
    }
}

/// The exponential moving average of the closes, seeded with the first one.
fn exponential_average(candles: &[(f64, Candle)], period: usize) -> Vec<(f64, f64)> {
    let k = 2.0 / (period as f64 + 1.0);
    let mut out = Vec::with_capacity(candles.len());
    let mut average = None;
    for &(x, candle) in candles {
        let next = average.map_or(candle.close, |a: f64| a + k * (candle.close - a));
        average = Some(next);
        out.push((x, next));
    }
    out
}

impl ChartModel for ChartData {
    fn x_extent(&self) -> Option<(f64, f64)> {
        Some((self.candles.first()?.0, self.candles.last()?.0))
    }

    fn x_step(&self) -> Option<f64> {
        Some(self.step)
    }

    fn panes(&self) -> usize {
        2
    }

    fn pane_weight(&self, pane: usize) -> f32 {
        if pane == 0 {
            4.0
        } else {
            1.0
        }
    }

    fn y_axis(&self, pane: usize, x: &Axis) -> Option<Axis> {
        if pane == 0 {
            let candles = candle_extent(&self.candles[..], candles_within(&self.candles[..], x));
            let average = y_extent(&self.average[..], points_within(&self.average[..], x));
            let (low, high) = match (candles, average) {
                (Some((a, b)), Some((c, d))) => (a.min(c), b.max(d)),
                (one, other) => one.or(other)?,
            };
            return Some(Axis::number(low, high).padded(0.08));
        }
        let (_, most) = y_extent(&self.ticks[..], points_within(&self.ticks[..], x))?;
        Some(Axis::number(0.0, most * 1.15))
    }

    fn paint(&self, pane: usize, plot: &Plot, canvas: &mut Canvas<'_>, palette: &ChartPalette) {
        if pane == 0 {
            paint_candles(
                canvas,
                plot,
                &self.candles[..],
                self.step,
                CandleStyle::new(palette.up, palette.down),
            );
            paint_line(
                canvas,
                plot,
                &self.average[..],
                LineStyle::new(palette.line).with_width(1.5),
            );
            return;
        }
        let candles = &self.candles;
        paint_bars(
            canvas,
            plot,
            &self.ticks[..],
            self.step,
            BarStyle::new(),
            |index| {
                let up = candles.get(index).is_some_and(|(_, c)| c.close >= c.open);
                if up { palette.up } else { palette.down }.with_alpha(0.5)
            },
        );
    }

    fn describe(&self) -> String {
        let symbol = self.instrument.as_ref().map_or("", |i| i.symbol.as_str());
        let last = self.candles.last().map_or(0.0, |(_, c)| c.close);
        format!("{symbol}, candles, last {last}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_average_starts_at_the_first_close_and_follows_the_rest() {
        let candles: Vec<(f64, Candle)> = [10.0, 12.0, 14.0]
            .iter()
            .enumerate()
            .map(|(i, &c)| (i as f64, Candle::new(c, c, c, c)))
            .collect();

        let average = exponential_average(&candles, 3);

        assert_eq!(average[0], (0.0, 10.0));
        assert_eq!(average[1], (1.0, 11.0));
        assert_eq!(average[2], (2.0, 12.5));
    }
}

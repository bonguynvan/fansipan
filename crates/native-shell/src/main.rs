//! Fansipan in a native window drawn on the GPU (densa): the sandbox wired
//! to the data engine, the price board and a chart in a docking workspace.
//! The market is the trader's choice, or the one `FANSIPAN_SANDBOX_MARKET`
//! names; the sandbox's other settings read as the web shell's do.

mod board;
mod chart;
mod feed;
mod i18n;

use std::env;
use std::error::Error;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use densa::ui::{Action, ActionKind, Align, Style, Ui, WidgetId};
use densa::widgets::{
    Button, Chart, DataGrid, DockArea, DockAt, DockPanel, DockSide, Label, Panel,
};
use densa::window::{App, Host, WindowOptions};
use model::UnixNanos;
use sandbox::profiles::PROFILE_IDS;
use sandbox::{Clock, MarketProfile, SandboxDataClient, DEFAULT_BEAT};

use crate::board::{BoardModel, PriceLook};
use crate::chart::ChartData;
use crate::feed::Feed;
use crate::i18n::{Words, LANGUAGE_ENV};

const VENUE: &str = "sim";
const MARKET_ENV: &str = "FANSIPAN_SANDBOX_MARKET";
const CLOCK_ENV: &str = "FANSIPAN_SANDBOX_CLOCK";
const BEAT_ENV: &str = "FANSIPAN_SANDBOX_BEAT_MS";
/// What the chart shows at first: twenty minutes of candles.
const CHART_SPAN_SECONDS: f64 = 1200.0;

type Board = DataGrid<BoardModel>;

struct Terminal {
    words: Words,
    beat: Duration,
    real_hours: bool,
    /// A market named at start: no question asked.
    market: Option<String>,
    chooser: Option<WidgetId>,
    choices: Vec<(WidgetId, &'static str)>,
    feed: Option<Feed>,
    board: Option<WidgetId>,
    chart: Option<WidgetId>,
}

fn now() -> UnixNanos {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    UnixNanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

/// Every simulated market, by title, so none comes first for being the default.
fn markets() -> Vec<MarketProfile> {
    let mut profiles: Vec<MarketProfile> = PROFILE_IDS
        .iter()
        .filter_map(|id| MarketProfile::by_id(id, VENUE).ok())
        .collect();
    profiles.sort_by(|a, b| a.title.cmp(b.title));
    profiles
}

impl Terminal {
    fn show_chooser(&mut self, ui: &mut Ui) -> Result<(), Box<dyn Error>> {
        let column = Style::column()
            .gap(10.0)
            .padding(28.0)
            .align_items(Align::Start);
        let chooser = ui.add(ui.root(), Panel::new(), column)?;
        ui.add(
            chooser,
            Label::new(self.words.t("venue.title", &[])).with_size(20.0),
            Style::row(),
        )?;
        ui.add(
            chooser,
            Label::new(self.words.t("venue.intro", &[]))
                .wrapping()
                .dim(),
            Style::row(),
        )?;
        for profile in markets() {
            let button = ui.add(chooser, Button::new(profile.title), Style::row())?;
            self.choices.push((button, profile.id));
        }
        self.chooser = Some(chooser);
        Ok(())
    }

    /// Connects market `id` and lays out the workspace on it.
    fn open(&mut self, ui: &mut Ui, host: &mut Host<'_>, id: &str) -> Result<(), Box<dyn Error>> {
        let profile = MarketProfile::by_id(id, VENUE)?;
        let clock = if self.real_hours {
            profile.real_clock()
        } else {
            Clock::Compressed { start: now() }
        };
        let client = SandboxDataClient::new(profile.venue_on(clock)).with_beat(self.beat);
        let instruments: Vec<_> = profile.listings.iter().map(|l| l.info.id.clone()).collect();
        self.feed = Some(Feed::start(Box::new(client), &instruments, host.waker())?);
        host.set_title(&format!("Fansipan · {}", profile.title));

        let root = ui.root();
        let dock = ui.add(root, DockArea::new(), DockArea::style().padding(0.0))?;
        let board_spec =
            DockPanel::new(self.words.t("screen.price-board.title", &[])).with_key("board");
        let board_panel = DockArea::add_panel(ui, dock, Panel::new(), board_spec, DockAt::First)?;
        let look = PriceLook::of(profile.id);
        let board = DataGrid::new(BoardModel::new(&self.words, look)).with_frozen_columns(1);
        self.board = Some(ui.add(board_panel, board, Board::style())?);
        let chart_spec = DockPanel::new(self.words.t("screen.chart.title", &[])).with_key("chart");
        let below = DockAt::Beside(board_panel, DockSide::Bottom);
        let chart_panel = DockArea::add_panel(ui, dock, Panel::new(), chart_spec, below)?;
        let chart = Chart::new(ChartData::new(look.scale), CHART_SPAN_SECONDS);
        self.chart = Some(ui.add(chart_panel, chart, Chart::<ChartData>::style())?);
        Ok(())
    }

    /// The chart follows the board's row `row`.
    fn follow(&self, ui: &mut Ui, row: usize) {
        let (Some(board), Some(chart)) = (self.board, self.chart) else {
            return;
        };
        let Some(instrument) = ui
            .widget::<Board>(board)
            .and_then(|b| b.model().instrument(row))
            .cloned()
        else {
            return;
        };
        let data = self.feed.as_ref().and_then(Feed::engine);
        ui.update::<Chart<ChartData>, _>(chart, |chart, cx| {
            let model = chart.model_mut(cx);
            model.follow(instrument);
            if let Some(data) = &data {
                model.refresh(data);
            }
        });
    }
}

impl App for Terminal {
    fn build(&mut self, ui: &mut Ui, host: &mut Host<'_>) -> Result<(), Box<dyn Error>> {
        match self.market.take() {
            Some(id) => self.open(ui, host, &id),
            None => self.show_chooser(ui),
        }
    }

    fn action(&mut self, ui: &mut Ui, host: &mut Host<'_>, action: Action) {
        if let Some(&(_, id)) = self
            .choices
            .iter()
            .find(|(button, _)| *button == action.source)
        {
            if action.kind == ActionKind::Clicked {
                if let Some(chooser) = self.chooser.take() {
                    ui.remove(chooser);
                }
                self.choices.clear();
                if let Err(e) = self.open(ui, host, id) {
                    eprintln!("could not open {id}: {e}");
                    host.close();
                }
            }
            return;
        }
        if Some(action.source) == self.board {
            if let ActionKind::Selected(row) = action.kind {
                self.follow(ui, row);
            }
        }
    }

    fn frame(&mut self, ui: &mut Ui, _: &mut Host<'_>) -> bool {
        let (Some(feed), Some(board), Some(chart)) = (&self.feed, self.board, self.chart) else {
            return false;
        };
        let now = Instant::now();
        if let Some(data) = feed.take_changes() {
            ui.update::<Board, _>(board, |grid, cx| grid.model_mut(cx).refresh(&data, now));
            let first = ui
                .widget::<Board>(board)
                .and_then(|b| b.model().instrument(0))
                .cloned();
            ui.update::<Chart<ChartData>, _>(chart, |chart, cx| {
                let model = chart.model_mut(cx);
                if model.instrument().is_none() {
                    if let Some(first) = first {
                        model.follow(first);
                    }
                }
                model.refresh(&data);
            });
        }
        // A lit price keeps painting until it has faded.
        ui.update::<Board, _>(board, |grid, cx| grid.model_mut(cx).tick(now))
            .unwrap_or(false)
    }
}

fn beat() -> Result<Duration, String> {
    match env::var(BEAT_ENV) {
        Err(_) => Ok(DEFAULT_BEAT),
        Ok(raw) => raw
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|ms| (1..=60_000).contains(ms))
            .map(Duration::from_millis)
            .ok_or_else(|| {
                format!(
                    "{BEAT_ENV}: \"{raw}\" is not a whole number of milliseconds from 1 to 60000"
                )
            }),
    }
}

fn main() -> ExitCode {
    let market = env::var(MARKET_ENV).ok().filter(|id| !id.trim().is_empty());
    if let Some(id) = &market {
        if let Err(e) = MarketProfile::by_id(id.trim(), VENUE) {
            eprintln!("{MARKET_ENV}: {e}");
            return ExitCode::FAILURE;
        }
    }
    let beat = match beat() {
        Ok(beat) => beat,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let terminal = Terminal {
        words: Words::new(&env::var(LANGUAGE_ENV).unwrap_or_default()),
        beat,
        real_hours: env::var(CLOCK_ENV).is_ok_and(|c| c.trim() == "real"),
        market: market.map(|id| id.trim().to_string()),
        chooser: None,
        choices: Vec::new(),
        feed: None,
        board: None,
        chart: None,
    };
    match densa::window::run(
        WindowOptions::new("Fansipan").with_size(1400.0, 860.0),
        terminal,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Fansipan could not start: {e}");
            ExitCode::FAILURE
        }
    }
}

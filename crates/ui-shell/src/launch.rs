//! Which venue the terminal runs against. A build with one venue connects
//! it before the window opens. A build that offers several (the sandbox's
//! markets, an adapter's paper and live accounts) asks the trader at first
//! launch, remembers the answer, and lets them change it later: the change
//! restarts the terminal, since the engines, the journal, and the screens
//! all belong to one venue for the life of the process.

use std::sync::mpsc::Receiver;
use std::sync::{Mutex, MutexGuard};

use bus::{Bus, Event};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::workspace::Store;
use crate::{connect_engines, flush, logbook, pump, settle, start_journal, Shell, Venue};

/// The settings key that remembers the trader's choice.
const CHOICE_KEY: &str = "venue.choice";
/// Sent to the window once the venue's engines answer commands, so a
/// page that loaded while it was being opened can show the workspace.
const STARTED_EVENT: &str = "venue-started";

/// One venue a trader can choose to run against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    id: String,
    title: String,
}

impl Choice {
    /// `id` is what the opener is called with and what settings remember;
    /// `title` is shown to the trader as it is.
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
        }
    }
}

pub(crate) type Opener = Box<dyn Fn(&str) -> Result<Venue, String> + Send + Sync>;

/// Commands that need the engines work only once `Running`: the shell is
/// managed before the stage says so.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    /// Waiting for the trader; `problem` says why the last attempt failed.
    Choosing {
        problem: Option<String>,
    },
    Opening,
    Running(String),
}

/// Managed state: the choices offered and where the launch stands.
pub(crate) struct Launch {
    choices: Vec<Choice>,
    opener: Option<Opener>,
    stage: Mutex<Stage>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ChoiceDto {
    id: String,
    title: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct LaunchDto {
    /// "choosing", "opening", or "running". The workspace loads only once
    /// running; while choosing, the window shows the choices.
    stage: &'static str,
    /// Empty when the build runs one venue, so there is nothing to change.
    choices: Vec<ChoiceDto>,
    current: Option<String>,
    /// Why the last venue tried did not open, for the trader.
    problem: Option<String>,
}

fn guard<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, String> {
    mutex
        .lock()
        .map_err(|_| "the venue's state is unavailable after an internal error; restart".into())
}

impl Launch {
    /// A build with one venue, connected before the window opens; it runs
    /// once [`start`] has managed it.
    pub(crate) fn single() -> Self {
        Self {
            choices: Vec::new(),
            opener: None,
            stage: Mutex::new(Stage::Opening),
        }
    }

    /// A build that offers `choices` and opens the chosen one with `opener`.
    pub(crate) fn choosing(choices: Vec<Choice>, opener: Opener) -> Self {
        Self {
            choices,
            opener: Some(opener),
            stage: Mutex::new(Stage::Choosing { problem: None }),
        }
    }

    fn offers(&self, id: &str) -> bool {
        self.choices.iter().any(|choice| choice.id == id)
    }

    fn state(&self) -> Result<LaunchDto, String> {
        let (stage, current, problem) = match &*guard(&self.stage)? {
            Stage::Choosing { problem } => ("choosing", None, problem.clone()),
            Stage::Opening => ("opening", None, None),
            Stage::Running(id) => ("running", Some(id.clone()), None),
        };
        Ok(LaunchDto {
            stage,
            choices: self
                .choices
                .iter()
                .map(|c| ChoiceDto {
                    id: c.id.clone(),
                    title: c.title.clone(),
                })
                .collect(),
            current,
            problem,
        })
    }

    /// Opens `id` on a bus of its own and connects the engines to it. Only
    /// one attempt runs at a time, and only while choosing: the first venue
    /// to open is the process's venue. A failed attempt goes back to
    /// choosing, saying why.
    fn open(&self, id: &str) -> Result<(Shell, Receiver<Event>), String> {
        if !self.offers(id) {
            return Err(format!("\"{id}\" is not one of the venues offered"));
        }
        let opener = self
            .opener
            .as_ref()
            .ok_or("this terminal runs one venue; there is nothing to choose")?;
        {
            let mut stage = guard(&self.stage)?;
            match *stage {
                Stage::Choosing { .. } => *stage = Stage::Opening,
                Stage::Opening => return Err("a venue is being opened".into()),
                Stage::Running(_) => {
                    return Err(
                        "a venue is already running; changing it restarts the terminal".into(),
                    )
                }
            }
        }
        let bus = Bus::new();
        let events = bus.subscribe();
        let opened = opener(id).and_then(|venue| {
            connect_engines(&bus, venue).map_err(|e| format!("could not connect to the venue: {e}"))
        });
        if let Err(problem) = &opened {
            *guard(&self.stage)? = Stage::Choosing {
                problem: Some(problem.clone()),
            };
        }
        opened.map(|shell| (shell, events))
    }

    fn mark_running(&self, id: &str) -> Result<(), String> {
        *guard(&self.stage)? = Stage::Running(id.to_string());
        Ok(())
    }
}

/// The remembered choice, if it is still one the build offers.
fn saved_choice(saved: Option<Value>, launch: &Launch) -> Option<String> {
    saved?
        .as_str()
        .filter(|id| launch.offers(id))
        .map(str::to_string)
}

/// Starts what runs once a venue is connected (the journal, the shell's
/// state for the commands, the threads that feed the screens), then marks
/// the venue `id` running and tells the window.
pub(crate) fn start(app: &AppHandle, shell: Shell, events: Receiver<Event>, id: &str) {
    logbook::info(&format!("venue {} ({id})", shell.venue));
    match app.path().app_data_dir() {
        Ok(data_dir) => {
            if let Err(e) = start_journal(&shell, &data_dir.join("journal")) {
                logbook::error(&format!("journal: {e}"));
            }
        }
        Err(e) => logbook::error(&format!("journal: no data folder: {e}")),
    }
    app.manage(shell);
    pump(app.clone(), events);
    flush(app.clone());
    settle(app.clone());
    if let Err(e) = app.state::<Launch>().mark_running(id) {
        logbook::error(&format!("venue {id}: {e}"));
    }
    if let Err(e) = app.emit(STARTED_EVENT, id) {
        logbook::warn(&format!("could not tell the window the venue started: {e}"));
    }
}

/// At start, opens the remembered choice. Without one, or when it no
/// longer opens, the window asks, saying why.
pub(crate) fn resume(app: &AppHandle) {
    let launch = app.state::<Launch>();
    let Some(id) = saved_choice(app.state::<Store>().read(CHOICE_KEY), &launch) else {
        return;
    };
    match launch.open(&id) {
        Ok((shell, events)) => start(app, shell, events, &id),
        Err(e) => logbook::error(&format!("could not open the remembered venue {id}: {e}")),
    }
}

#[tauri::command]
pub(crate) fn launch_state(launch: State<'_, Launch>) -> Result<LaunchDto, String> {
    launch.state()
}

/// The trader's first choice: opened at once, then remembered.
#[tauri::command]
pub(crate) fn choose_venue(
    app: AppHandle,
    launch: State<'_, Launch>,
    store: State<'_, Store>,
    id: String,
) -> Result<(), String> {
    let (shell, events) = launch.open(&id)?;
    if let Err(e) = store.write(CHOICE_KEY, Value::from(id.as_str())) {
        logbook::warn(&format!(
            "the venue chosen will be asked again next start: {e}"
        ));
    }
    start(&app, shell, events, &id);
    Ok(())
}

/// Another venue, from the menu: remembered, then the terminal restarts on
/// it, once the window has saved what it holds.
#[tauri::command]
pub(crate) fn switch_venue(
    app: AppHandle,
    launch: State<'_, Launch>,
    store: State<'_, Store>,
    id: String,
) -> Result<(), String> {
    if !launch.offers(&id) {
        return Err(format!("\"{id}\" is not one of the venues offered"));
    }
    store
        .write(CHOICE_KEY, Value::from(id.as_str()))
        .map_err(|e| format!("could not remember the venue: {e}"))?;
    logbook::info(&format!("restarting on venue {id}"));
    crate::workspace::restart(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Market, RiskLimits};
    use sandbox::{MarketProfile, SandboxDataClient, SandboxExecutionClient};

    fn sandbox(id: &str) -> Result<Venue, String> {
        let profile = MarketProfile::by_id(id, "sim")?;
        let sim = profile.venue();
        Ok(Venue::new(
            "sim",
            Market::new(profile.id, profile.title, profile.currency),
            Box::new(SandboxDataClient::new(sim.clone())),
            Box::new(SandboxExecutionClient::new(sim)),
            RiskLimits {
                max_order_quantity: profile.max_order_quantity,
                max_order_notional: profile.max_order_notional,
            },
        ))
    }

    fn markets() -> Launch {
        Launch::choosing(
            vec![
                Choice::new("us", "United States"),
                Choice::new("vn", "Vietnam"),
            ],
            Box::new(sandbox),
        )
    }

    fn stage(launch: &Launch) -> &'static str {
        launch.state().unwrap().stage
    }

    #[test]
    fn a_build_with_choices_asks_until_one_is_opened() {
        let launch = markets();
        assert_eq!(stage(&launch), "choosing");
        assert_eq!(launch.state().unwrap().choices.len(), 2);

        let (shell, _events) = launch.open("us").unwrap();

        assert_eq!(shell.market.id, "us");
        assert_eq!(stage(&launch), "opening");
    }

    #[test]
    fn the_venue_runs_only_once_marked_after_its_shell_is_managed() {
        let launch = markets();
        launch.open("us").unwrap();
        assert_eq!(launch.state().unwrap().current, None);

        launch.mark_running("us").unwrap();

        let state = launch.state().unwrap();
        assert_eq!(state.stage, "running");
        assert_eq!(state.current.as_deref(), Some("us"));
    }

    #[test]
    fn only_the_first_venue_opens() {
        let launch = markets();
        launch.open("us").unwrap();

        let while_opening = launch.open("vn").map(drop).unwrap_err();
        launch.mark_running("us").unwrap();
        let once_running = launch.open("vn").map(drop).unwrap_err();

        assert!(while_opening.contains("being opened"), "{while_opening}");
        assert!(once_running.contains("restarts"), "{once_running}");
        assert_eq!(launch.state().unwrap().current.as_deref(), Some("us"));
    }

    #[test]
    fn a_venue_not_offered_is_refused_and_the_choice_stays_open() {
        let launch = markets();

        let refused = launch.open("mars").map(drop).unwrap_err();

        assert!(refused.contains("mars"), "{refused}");
        assert_eq!(stage(&launch), "choosing");
        assert!(launch.open("vn").is_ok());
    }

    #[test]
    fn a_venue_that_fails_to_open_is_asked_again_saying_why() {
        let launch = Launch::choosing(
            vec![Choice::new("down", "Down"), Choice::new("us", "Up")],
            Box::new(|id| {
                if id == "down" {
                    Err("no route".into())
                } else {
                    sandbox(id)
                }
            }),
        );

        assert_eq!(launch.open("down").map(drop).unwrap_err(), "no route");

        let state = launch.state().unwrap();
        assert_eq!(state.stage, "choosing");
        assert_eq!(state.problem.as_deref(), Some("no route"));
        assert!(launch.open("us").is_ok());
    }

    #[test]
    fn a_saved_choice_counts_only_while_it_is_offered() {
        let launch = markets();

        assert_eq!(
            saved_choice(Some(Value::from("vn")), &launch).as_deref(),
            Some("vn")
        );
        assert_eq!(saved_choice(Some(Value::from("mars")), &launch), None);
        assert_eq!(saved_choice(Some(Value::from(3)), &launch), None);
        assert_eq!(saved_choice(None, &launch), None);
    }

    #[test]
    fn a_single_venue_offers_nothing_to_change() {
        let launch = Launch::single();

        let state = launch.state().unwrap();

        assert_eq!(state.stage, "opening");
        assert!(state.choices.is_empty());
        assert!(launch.open("sim").is_err());
    }
}

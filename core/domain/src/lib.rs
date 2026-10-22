//! Domain core for the Jupiter Chess.com crawler.
//!
//! [`Core`] is the single command and query layer used by every shell (the C ABI facade for the
//! WinUI app and the Rust CLI). It owns the crawl settings, the crawl thread and its status.

pub mod chesscom;
pub mod control;
pub mod countries;
pub mod engine;
pub mod fide;
pub mod gates;
pub mod month;
pub mod names;
pub mod output;
pub mod pgn;
pub mod settings;
pub mod status;
pub mod store;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::chesscom::{Transport, UreqTransport};
use crate::control::Control;
use crate::settings::{Settings, SettingsError};
use crate::status::{Event, RunState, Shared, Status};

pub use crate::month::Month;

pub const SETTINGS_FILE_VERSION: u32 = 1;

pub struct CoreConfig {
    /// Writable folder for settings and the state database.
    pub data_dir: PathBuf,
    /// Output folder used when the settings do not name one.
    pub default_output_dir: PathBuf,
}

#[derive(Debug)]
pub enum CoreError {
    Io(String),
    Settings(SettingsError),
    Store(String),
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreError::Io(m) | CoreError::Store(m) => f.write_str(m),
            CoreError::Settings(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SettingsFile {
    version: u32,
    settings: Settings,
}

type TransportFactory = Box<dyn Fn() -> Box<dyn Transport> + Send + Sync>;

struct Worker {
    control: Arc<Control>,
    handle: JoinHandle<()>,
}

pub struct Core {
    data_dir: PathBuf,
    settings: Mutex<Settings>,
    shared: Arc<Shared>,
    worker: Mutex<Option<Worker>>,
    transport: TransportFactory,
}

impl Core {
    pub fn open(config: CoreConfig) -> Result<Core, CoreError> {
        Core::open_with_transport(config, Box::new(|| Box::new(UreqTransport::default())))
    }

    pub fn open_with_transport(config: CoreConfig, transport: TransportFactory) -> Result<Core, CoreError> {
        fs::create_dir_all(&config.data_dir)
            .map_err(|e| CoreError::Io(format!("Cannot create {}: {e}", config.data_dir.display())))?;
        let shared = Arc::new(Shared::default());
        let mut settings = Settings::default();
        let path = settings_path(&config.data_dir);
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<SettingsFile>(&text) {
                Ok(f) if f.version <= SETTINGS_FILE_VERSION => settings = f.settings,
                Ok(f) => shared.warn(format!(
                    "{} was written by a newer version (format {}); defaults are in use and the file is left unchanged.",
                    path.display(),
                    f.version
                )),
                Err(e) => shared.warn(format!(
                    "{} cannot be read ({e}); defaults are in use and the file is left unchanged until settings are saved.",
                    path.display()
                )),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => shared.warn(format!("{} cannot be read: {e}", path.display())),
        }
        if settings.output_folder.trim().is_empty() {
            settings.output_folder = config.default_output_dir.to_string_lossy().into_owned();
        }
        // Surface FIDE coverage before the first crawl.
        let (catalog, _) = fide::FideCatalog::scan(&settings.fide_folders);
        shared.update(|s| {
            s.fide_first = catalog.earliest();
            s.fide_last = catalog.latest();
            s.phase = "Not started".into();
        });
        let core = Core {
            data_dir: config.data_dir,
            settings: Mutex::new(settings),
            shared,
            worker: Mutex::new(None),
            transport,
        };
        core.refresh_counts();
        Ok(core)
    }

    /// Shows stored counts for the current settings while no crawl is running.
    fn refresh_counts(&self) {
        let settings = self.settings();
        if let Ok(store) = store::Store::open(&self.state_path()) {
            let (known, resolved) = store.account_counts().unwrap_or_default();
            let total = store.game_count(None, settings.rating_source.code()).unwrap_or_default();
            let identifies = settings.identifies();
            self.shared.update(|s| {
                s.known_players = if identifies { known } else { 0 };
                s.resolved_players = resolved;
                s.total_games = total;
            });
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn state_path(&self) -> PathBuf {
        engine::state_path(&self.data_dir)
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Validates and saves settings. A running crawl restarts with them.
    pub fn update_settings(&self, settings: Settings) -> Result<Settings, CoreError> {
        let settings = settings.validated().map_err(CoreError::Settings)?;
        let file = SettingsFile { version: SETTINGS_FILE_VERSION, settings: settings.clone() };
        let json = serde_json::to_string_pretty(&file).map_err(|e| CoreError::Io(e.to_string()))?;
        write_replacing(&settings_path(&self.data_dir), json.as_bytes())?;
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
        if self.status().state.is_active() {
            self.shared.info("Settings saved; restarting the crawl with them.");
            self.stop();
            self.start()?;
        } else {
            self.shared.info("Settings saved.");
            self.refresh_counts();
        }
        Ok(settings)
    }

    /// Starts the crawl, or does nothing if one is running. A crawl that is still stopping is
    /// allowed to finish first.
    pub fn start(&self) -> Result<(), CoreError> {
        let settings = self.settings().validated().map_err(CoreError::Settings)?;
        let mut worker = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        let previous = worker.take();
        if let Some(w) = &previous
            && !w.control.is_stopped()
            && !w.handle.is_finished()
        {
            *worker = previous;
            return Ok(());
        }
        let control = Arc::new(Control::default());
        let shared = self.shared.clone();
        let transport = (self.transport)();
        let state_path = self.state_path();
        let thread_control = control.clone();
        self.shared.update(|s| {
            s.state = RunState::Running;
            s.phase = "Starting".into();
            s.last_error.clear();
        });
        let handle = std::thread::Builder::new()
            .name("chesscom-crawl".into())
            .spawn(move || {
                if let Some(previous) = previous {
                    let _ = previous.handle.join();
                }
                if !thread_control.is_stopped() {
                    engine::run(settings, state_path, transport, thread_control, shared);
                }
            })
            .map_err(|e| CoreError::Io(format!("Cannot start the crawl thread: {e}")))?;
        *worker = Some(Worker { control, handle });
        Ok(())
    }

    /// Requests a stop. Returns immediately; the status shows Stopping until the thread ends.
    pub fn stop(&self) {
        let worker = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = worker.as_ref()
            && !w.handle.is_finished()
        {
            w.control.request_stop();
            self.shared.update(|s| {
                s.state = RunState::Stopping;
                s.phase = "Stopping".into();
            });
        }
    }

    pub fn status(&self) -> Status {
        self.shared.snapshot()
    }

    pub fn poll_events(&self, max: usize) -> Vec<Event> {
        self.shared.drain_events(max)
    }

    /// Stops the crawl and waits up to `timeout` for the thread to end.
    pub fn shutdown(&self, timeout: Duration) -> bool {
        self.stop();
        let worker = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(w) = worker else { return true };
        let deadline = Instant::now() + timeout;
        while !w.handle.is_finished() {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = w.handle.join();
        true
    }

    /// Rewrites every month's collection files from collected games under the current settings.
    pub fn export_all(&self) -> Result<usize, CoreError> {
        let settings = self.settings();
        let store = store::Store::open(&self.state_path()).map_err(|e| CoreError::Store(e.to_string()))?;
        let months =
            store.months_with_games(settings.rating_source.code()).map_err(|e| CoreError::Store(e.to_string()))?;
        for m in &months {
            output::export_month(&store, &settings, *m).map_err(|e| CoreError::Io(e.to_string()))?;
        }
        Ok(months.len())
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        self.shutdown(Duration::from_secs(30));
    }
}

pub fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

fn write_replacing(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let err = |e: std::io::Error| CoreError::Io(format!("Cannot write {}: {e}", path.display()));
    let tmp = path.with_extension("json.partial");
    {
        let mut f = fs::File::create(&tmp).map_err(err)?;
        f.write_all(bytes).map_err(err)?;
        f.sync_all().map_err(err)?;
    }
    fs::rename(&tmp, path).map_err(err)
}

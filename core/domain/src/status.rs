//! Status snapshot and activity events shared between the crawl thread and the command layer.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::month::Month;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunState {
    #[default]
    Idle,
    Running,
    /// Running, but deliberately waiting: a server back-off or the next month to complete.
    Waiting,
    Stopping,
    Completed,
    Failed,
}

impl RunState {
    pub fn is_active(self) -> bool {
        matches!(self, RunState::Running | RunState::Waiting | RunState::Stopping)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub state: RunState,
    pub phase: String,
    pub month: Option<Month>,
    pub month_players_done: u32,
    pub month_players_total: u32,
    pub month_games: u32,
    pub total_games: u64,
    pub months_done: u32,
    pub months_total: u32,
    pub known_players: u32,
    pub resolved_players: u32,
    pub requests: u64,
    pub last_request_unix: i64,
    pub waiting_until_unix: i64,
    pub last_error: String,
    pub fide_first: Option<Month>,
    pub fide_last: Option<Month>,
    pub fide_index_list: Option<Month>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub unix_ms: i64,
    pub level: Level,
    pub message: String,
}

const EVENT_CAPACITY: usize = 1000;

#[derive(Default)]
pub struct Shared {
    status: Mutex<Status>,
    events: Mutex<VecDeque<Event>>,
}

impl Shared {
    pub fn snapshot(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.status.lock().unwrap_or_else(|e| e.into_inner()));
    }

    pub fn log(&self, level: Level, message: impl Into<String>) {
        let unix_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        if events.len() >= EVENT_CAPACITY {
            events.pop_front();
        }
        events.push_back(Event { unix_ms, level, message: message.into() });
    }

    pub fn info(&self, message: impl Into<String>) {
        self.log(Level::Info, message);
    }

    pub fn warn(&self, message: impl Into<String>) {
        self.log(Level::Warning, message);
    }

    /// Removes and returns up to `max` of the oldest pending events.
    pub fn drain_events(&self, max: usize) -> Vec<Event> {
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        let n = max.min(events.len());
        events.drain(..n).collect()
    }
}

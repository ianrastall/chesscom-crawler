//! Chess.com published-data API client.
//!
//! Requests are strictly serial: one connection, one request at a time, with a configurable
//! minimum interval between request starts. Rate-limit and server errors back off exponentially
//! (honouring `Retry-After`) and retry indefinitely; only a stop request ends a wait.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::control::{Control, Stopped};
use crate::month::{Month, unix_now};
use crate::status::{RunState, Shared};

pub const API_ROOT: &str = "https://api.chess.com/pub";

pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub retry_after: Option<u64>,
}

/// One blocking GET. Implemented over ureq in production and by canned responses in tests.
pub trait Transport: Send {
    fn get(&mut self, url: &str, user_agent: &str) -> Result<HttpResponse, String>;
}

pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(90)))
            .http_status_as_error(false)
            .max_idle_connections(1)
            .max_idle_connections_per_host(1)
            .build()
            .into();
        UreqTransport { agent }
    }
}

impl Transport for UreqTransport {
    fn get(&mut self, url: &str, user_agent: &str) -> Result<HttpResponse, String> {
        let mut response = self
            .agent
            .get(url)
            .header("User-Agent", user_agent)
            .header("Accept", "application/json")
            .call()
            .map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let body =
            response.body_mut().with_config().limit(256 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
        Ok(HttpResponse { status, body, retry_after })
    }
}

const FIRST_BACKOFF: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);

pub struct Client {
    transport: Box<dyn Transport>,
    interval: Duration,
    user_agent: String,
    last_start: Option<Instant>,
    control: Arc<Control>,
    shared: Arc<Shared>,
}

pub fn user_agent(contact: &str) -> String {
    let base = format!("JupiterChesscomCrawler/{} (serial requests", env!("CARGO_PKG_VERSION"));
    if contact.is_empty() { format!("{base})") } else { format!("{base}; contact: {contact})") }
}

/// Chess.com usernames are ASCII letters, digits, underscores and hyphens.
pub fn valid_username(name: &str) -> bool {
    !name.is_empty() && name.len() <= 50 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl Client {
    pub fn new(
        transport: Box<dyn Transport>,
        interval_seconds: f64,
        contact: &str,
        control: Arc<Control>,
        shared: Arc<Shared>,
    ) -> Client {
        Client {
            transport,
            interval: Duration::from_secs_f64(interval_seconds.max(0.0)),
            user_agent: user_agent(contact),
            last_start: None,
            control,
            shared,
        }
    }

    /// GETs `url`. `Ok(None)` means the resource does not exist (404/410).
    pub fn get(&mut self, url: &str) -> Result<Option<Vec<u8>>, Stopped> {
        let mut backoff = FIRST_BACKOFF;
        loop {
            if let Some(last) = self.last_start {
                let due = last + self.interval;
                let now = Instant::now();
                if due > now {
                    self.control.sleep(due - now)?;
                }
            }
            self.control.check()?;
            self.last_start = Some(Instant::now());
            let result = self.transport.get(url, &self.user_agent);
            self.shared.update(|s| {
                s.requests += 1;
                s.last_request_unix = unix_now();
            });
            let (wait, reason) = match result {
                Ok(r) if r.status == 200 => return Ok(Some(r.body)),
                Ok(r) if r.status == 404 || r.status == 410 => return Ok(None),
                Ok(r) if r.status == 429 || r.status == 403 => {
                    let wait = r.retry_after.map(Duration::from_secs).unwrap_or(backoff).max(backoff);
                    (wait, format!("Chess.com returned {} (rate limited)", r.status))
                }
                Ok(r) => (backoff, format!("Chess.com returned {}", r.status)),
                Err(e) => (backoff, format!("Request failed: {e}")),
            };
            let wait = wait.min(MAX_BACKOFF);
            let until = unix_now() + wait.as_secs() as i64;
            self.shared.warn(format!("{reason} for {url}; waiting {} before retrying.", describe(wait)));
            self.shared.update(|s| {
                s.state = RunState::Waiting;
                s.waiting_until_unix = until;
                s.last_error = reason.clone();
            });
            let slept = self.control.sleep(wait);
            self.shared.update(|s| {
                s.state = RunState::Running;
                s.waiting_until_unix = 0;
            });
            slept?;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&mut self, url: &str) -> Result<Option<T>, Stopped> {
        let Some(body) = self.get(url)? else { return Ok(None) };
        match serde_json::from_slice(&body) {
            Ok(v) => Ok(Some(v)),
            Err(e) => {
                self.shared.warn(format!("Unreadable response from {url}: {e}"));
                Ok(None)
            }
        }
    }

    pub fn titled(&mut self, title: &str) -> Result<Option<Vec<String>>, Stopped> {
        #[derive(Deserialize)]
        struct Titled {
            players: Vec<String>,
        }
        Ok(self.get_json::<Titled>(&format!("{API_ROOT}/titled/{title}"))?.map(|t| t.players))
    }

    pub fn profile(&mut self, username: &str) -> Result<Option<Profile>, Stopped> {
        self.get_json(&format!("{API_ROOT}/player/{}", username.to_ascii_lowercase()))
    }

    pub fn archives(&mut self, username: &str) -> Result<Option<Vec<Month>>, Stopped> {
        #[derive(Deserialize)]
        struct Archives {
            archives: Vec<String>,
        }
        let url = format!("{API_ROOT}/player/{}/games/archives", username.to_ascii_lowercase());
        Ok(self
            .get_json::<Archives>(&url)?
            .map(|a| a.archives.iter().filter_map(|u| month_from_archive_url(u)).collect()))
    }

    pub fn month_games(&mut self, username: &str, month: Month) -> Result<Option<Vec<Game>>, Stopped> {
        #[derive(Deserialize)]
        struct Games {
            games: Vec<Game>,
        }
        let url = format!("{API_ROOT}/player/{}/games/{}", username.to_ascii_lowercase(), month.api_path());
        Ok(self.get_json::<Games>(&url)?.map(|g| g.games))
    }
}

pub fn describe(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 120 { format!("{} min", s / 60) } else { format!("{s} s") }
}

fn month_from_archive_url(url: &str) -> Option<Month> {
    let mut parts = url.trim_end_matches('/').rsplit('/');
    let m = parts.next()?;
    let y = parts.next()?;
    Month::new(y.parse().ok()?, m.parse().ok()?)
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub username: String,
    pub name: Option<String>,
    pub title: Option<String>,
    pub country: String,
    pub status: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Side {
    pub rating: i32,
    pub result: String,
    pub username: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Game {
    pub url: String,
    pub pgn: String,
    pub time_control: String,
    pub end_time: i64,
    pub rated: bool,
    pub uuid: String,
    pub initial_setup: String,
    pub time_class: String,
    pub rules: String,
    pub white: Side,
    pub black: Side,
}

impl Game {
    /// A stable identity for de-duplication: the UUID, else the game URL.
    pub fn key(&self) -> &str {
        if self.uuid.is_empty() { &self.url } else { &self.uuid }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    type Routes = HashMap<String, VecDeque<(u16, String)>>;

    /// Serves canned responses by URL and records each request's start time.
    #[derive(Clone, Default)]
    pub struct FakeTransport {
        pub routes: Arc<Mutex<Routes>>,
        pub log: Arc<Mutex<Vec<(String, Instant)>>>,
    }

    impl FakeTransport {
        pub fn route(&self, url: &str, status: u16, body: &str) {
            self.routes.lock().unwrap().entry(url.to_string()).or_default().push_back((status, body.to_string()));
        }

        pub fn requested(&self) -> Vec<String> {
            self.log.lock().unwrap().iter().map(|(u, _)| u.clone()).collect()
        }
    }

    impl Transport for FakeTransport {
        fn get(&mut self, url: &str, _ua: &str) -> Result<HttpResponse, String> {
            self.log.lock().unwrap().push((url.to_string(), Instant::now()));
            let mut routes = self.routes.lock().unwrap();
            let (status, body) = match routes.get_mut(url) {
                Some(q) if q.len() > 1 => q.pop_front().unwrap(),
                Some(q) => q.front().cloned().unwrap(),
                None => (404, String::new()),
            };
            Ok(HttpResponse { status, body: body.into_bytes(), retry_after: Some(0) })
        }
    }

    #[test]
    fn requests_are_spaced_and_retried() {
        let fake = FakeTransport::default();
        let url = format!("{API_ROOT}/titled/GM");
        fake.route(&url, 500, "");
        fake.route(&url, 200, r#"{"players":["a","b"]}"#);
        let control = Arc::new(Control::default());
        let shared = Arc::new(Shared::default());
        let mut client = Client::new(Box::new(fake.clone()), 1.0, "", control.clone(), shared.clone());
        let profile_url = format!("{API_ROOT}/player/x");
        fake.route(&profile_url, 200, r#"{"username":"x","name":"X Y"}"#);
        assert_eq!(client.profile("X").unwrap().unwrap().name.as_deref(), Some("X Y"));
        assert!(client.profile("missing").unwrap().is_none());
        let log = fake.log.lock().unwrap().clone();
        assert!(log[1].1 - log[0].1 >= Duration::from_millis(990));
        assert_eq!(shared.snapshot().requests, 2);

        // A stop during a back-off ends the request promptly.
        let stopper = control.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1200));
            stopper.request_stop();
        });
        assert_eq!(client.titled("GM").unwrap_err(), Stopped);
    }

    #[test]
    fn parses_archive_urls_and_usernames() {
        assert_eq!(
            month_from_archive_url("https://api.chess.com/pub/player/hikaru/games/2026/09"),
            Month::parse("2026-09")
        );
        assert!(valid_username("Magnus_Carlsen-1"));
        assert!(!valid_username("../x"));
        assert!(user_agent("me@example.org").contains("contact: me@example.org"));
    }
}

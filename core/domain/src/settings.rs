//! Crawl settings: what to collect, where from and how politely.

use serde::{Deserialize, Serialize};

use crate::month::{CHESSCOM_FIRST_MONTH, Month};

/// Chess.com title lists that can seed the crawl, in display order.
pub const TITLES: [&str; 10] = ["GM", "WGM", "IM", "WIM", "FM", "WFM", "CM", "WCM", "NM", "WNM"];

pub const MIN_REQUEST_INTERVAL: f64 = 1.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Which ratings the floors test and the collections carry.
    pub rating_source: RatingSource,
    /// With Chess.com ratings: also require both players to be identified as FIDE players.
    /// FIDE ratings always require identification.
    pub require_identification: bool,
    /// Each floor writes its own collection; both players' ratings must meet it.
    pub floors: Vec<u16>,
    pub time_classes: TimeClasses,
    pub standard_only: bool,
    pub rated_only: bool,
    pub checkmate_only: bool,
    pub exclude_fair_play_closures: bool,
    /// When identifying players: also identify accounts that are not on any Chess.com title
    /// list (one profile request each).
    pub include_untitled_accounts: bool,
    pub months: MonthPlan,
    pub follow_new_months: bool,
    pub seed_titles: Vec<String>,
    /// Usernames crawled in addition to the title lists.
    pub extra_accounts: Vec<String>,
    pub discover_opponents: bool,
    pub fide_folders: Vec<String>,
    pub max_fide_list_age_months: u8,
    pub request_interval_seconds: f64,
    /// Optional contact details sent in the User-Agent header, as Chess.com requests.
    pub contact: String,
    /// With FIDE ratings: skip identifying an unknown opponent whose Chess.com rating in the game
    /// is this far below the lowest floor. Zero disables the check.
    pub prefilter_margin: u16,
    pub output_folder: String,
    pub remove_clock_comments: bool,
    pub start_on_launch: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeClasses {
    pub bullet: bool,
    pub blitz: bool,
    pub rapid: bool,
    pub daily: bool,
}

impl Default for TimeClasses {
    fn default() -> Self {
        TimeClasses { bullet: false, blitz: true, rapid: true, daily: true }
    }
}

impl TimeClasses {
    pub fn allows(&self, time_class: &str) -> bool {
        match time_class {
            "bullet" => self.bullet,
            "blitz" => self.blitz,
            "rapid" => self.rapid,
            "daily" => self.daily,
            _ => false,
        }
    }

    fn within(&self, other: &TimeClasses) -> bool {
        (!self.bullet || other.bullet)
            && (!self.blitz || other.blitz)
            && (!self.rapid || other.rapid)
            && (!self.daily || other.daily)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RatingSource {
    /// Chess.com usernames and the Chess.com ratings recorded with each game.
    #[default]
    Chesscom,
    /// FIDE names and FIDE standard ratings from the month's rating list.
    Fide,
}

impl RatingSource {
    pub fn code(self) -> &'static str {
        match self {
            RatingSource::Chesscom => "chesscom",
            RatingSource::Fide => "fide",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MonthMode {
    #[default]
    Continuous,
    Specific,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    #[default]
    Backward,
    Forward,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MonthPlan {
    pub mode: MonthMode,
    /// `YYYY-MM`; empty means the previous calendar month.
    pub start: String,
    pub direction: Direction,
    /// `YYYY-MM`; empty means no bound other than Chess.com's first month or the previous month.
    pub stop: String,
    /// Comma-separated months and `YYYY-MM..YYYY-MM` ranges.
    pub specific: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            rating_source: RatingSource::Chesscom,
            require_identification: false,
            floors: vec![2400],
            time_classes: TimeClasses::default(),
            standard_only: true,
            rated_only: true,
            checkmate_only: false,
            exclude_fair_play_closures: true,
            include_untitled_accounts: false,
            months: MonthPlan::default(),
            follow_new_months: true,
            seed_titles: ["GM", "WGM", "IM", "FM"].map(String::from).to_vec(),
            extra_accounts: Vec::new(),
            discover_opponents: true,
            fide_folders: vec![r"D:\dev\proj\fide".into(), r"D:\fide-archive\sqlite".into()],
            max_fide_list_age_months: 3,
            request_interval_seconds: 3.0,
            contact: String::new(),
            prefilter_margin: 400,
            output_folder: String::new(),
            remove_clock_comments: false,
            start_on_launch: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsError {
    pub field: &'static str,
    pub message: String,
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

fn invalid(field: &'static str, message: impl Into<String>) -> SettingsError {
    SettingsError { field, message: message.into() }
}

impl Settings {
    /// Whether players must be identified against the FIDE lists.
    pub fn identifies(&self) -> bool {
        self.rating_source == RatingSource::Fide || self.require_identification
    }

    pub fn min_floor(&self) -> u16 {
        self.floors.iter().copied().min().unwrap_or(0)
    }

    /// Validates the settings and returns a normalized copy (sorted floors, upper-case titles).
    pub fn validated(&self) -> Result<Settings, SettingsError> {
        let mut s = self.clone();
        s.floors.sort_unstable();
        s.floors.dedup();
        if s.floors.is_empty() {
            return Err(invalid("floors", "Enter at least one rating floor."));
        }
        if let Some(f) = s.floors.iter().find(|f| !(1000..=3000).contains(*f)) {
            return Err(invalid("floors", format!("{f} is outside 1000–3000.")));
        }
        let tc = s.time_classes;
        if !(tc.bullet || tc.blitz || tc.rapid || tc.daily) {
            return Err(invalid("time_classes", "Select at least one time class."));
        }
        s.seed_titles = s.seed_titles.iter().map(|t| t.trim().to_ascii_uppercase()).collect();
        s.seed_titles.retain(|t| !t.is_empty());
        s.seed_titles.sort_by_key(|t| TITLES.iter().position(|x| x == t).unwrap_or(usize::MAX));
        s.seed_titles.dedup();
        if let Some(t) = s.seed_titles.iter().find(|t| !TITLES.contains(&t.as_str())) {
            return Err(invalid("seed_titles", format!("{t} is not a Chess.com title list.")));
        }
        s.extra_accounts = s.extra_accounts.iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
        s.extra_accounts.sort_by_key(|a| a.to_ascii_lowercase());
        s.extra_accounts.dedup_by_key(|a| a.to_ascii_lowercase());
        if let Some(a) = s.extra_accounts.iter().find(|a| !crate::chesscom::valid_username(a)) {
            return Err(invalid("extra_accounts", format!("“{a}” is not a Chess.com username.")));
        }
        if s.seed_titles.is_empty() && s.extra_accounts.is_empty() {
            return Err(invalid("seed_titles", "Select at least one title list or add accounts."));
        }
        s.fide_folders = s.fide_folders.iter().map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect();
        if s.fide_folders.is_empty() && s.identifies() {
            return Err(invalid("fide_folders", "Enter at least one FIDE database folder."));
        }
        if !s.request_interval_seconds.is_finite()
            || !(MIN_REQUEST_INTERVAL..=3600.0).contains(&s.request_interval_seconds)
        {
            return Err(invalid("request_interval_seconds", format!("Use {MIN_REQUEST_INTERVAL}–3600 seconds.")));
        }
        if s.max_fide_list_age_months > 24 {
            return Err(invalid("max_fide_list_age_months", "Use 0–24 months."));
        }
        s.contact = s.contact.trim().to_string();
        if s.contact.chars().any(|c| c.is_control()) {
            return Err(invalid("contact", "Contact details cannot contain control characters."));
        }
        s.output_folder = s.output_folder.trim().to_string();
        if s.output_folder.is_empty() {
            return Err(invalid("output_folder", "Choose an output folder."));
        }
        s.schedule(Month::previous_utc())?;
        Ok(s)
    }

    /// The months to crawl, in order, given the latest complete month.
    pub fn schedule(&self, latest: Month) -> Result<Vec<Month>, SettingsError> {
        let plan = &self.months;
        let in_range = |m: Month| m >= CHESSCOM_FIRST_MONTH && m <= latest;
        match plan.mode {
            MonthMode::Specific => {
                let months = parse_month_list(&plan.specific).map_err(|e| invalid("months.specific", e))?;
                if months.is_empty() {
                    return Err(invalid("months.specific", "Enter at least one month."));
                }
                Ok(months.into_iter().filter(|m| in_range(*m)).collect())
            }
            MonthMode::Continuous => {
                let start = parse_optional(&plan.start, "months.start")?.unwrap_or(latest);
                let stop = parse_optional(&plan.stop, "months.stop")?;
                let mut months = Vec::new();
                let mut m = start.min(latest);
                match plan.direction {
                    Direction::Backward => {
                        let bound = stop.unwrap_or(CHESSCOM_FIRST_MONTH).max(CHESSCOM_FIRST_MONTH);
                        while m >= bound {
                            months.push(m);
                            m = m.prev();
                        }
                    }
                    Direction::Forward => {
                        m = m.max(CHESSCOM_FIRST_MONTH);
                        let bound = stop.unwrap_or(latest).min(latest);
                        while m <= bound {
                            months.push(m);
                            m = m.next();
                        }
                    }
                }
                Ok(months)
            }
        }
    }

    /// True when every game these settings would collect would also have been collected under
    /// `old`, so progress recorded under `old` remains valid.
    pub fn narrows(&self, old: &Settings) -> bool {
        self.rating_source == old.rating_source
            && (self.require_identification || !old.require_identification)
            && self.time_classes.within(&old.time_classes)
            && (self.standard_only || !old.standard_only)
            && (self.rated_only || !old.rated_only)
            && (self.checkmate_only || !old.checkmate_only)
            && (self.exclude_fair_play_closures || !old.exclude_fair_play_closures)
            && (!self.include_untitled_accounts || old.include_untitled_accounts)
            && self.min_floor() >= old.min_floor()
            && (old.prefilter_margin == 0
                || (self.prefilter_margin != 0 && self.prefilter_margin <= old.prefilter_margin))
            && (!self.identifies()
                || (self.max_fide_list_age_months <= old.max_fide_list_age_months
                    && self.fide_folders == old.fide_folders))
    }
}

fn parse_optional(text: &str, field: &'static str) -> Result<Option<Month>, SettingsError> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    Month::parse(text).map(Some).ok_or_else(|| invalid(field, format!("“{}” is not a YYYY-MM month.", text.trim())))
}

/// Parses `2026-01..2026-06, 2025-11`. Ranges expand in the order written; duplicates are dropped.
pub fn parse_month_list(text: &str) -> Result<Vec<Month>, String> {
    let mut out: Vec<Month> = Vec::new();
    let mut push = |m: Month| {
        if !out.contains(&m) {
            out.push(m);
        }
    };
    for item in text.split([',', ';', '\n']).map(str::trim).filter(|s| !s.is_empty()) {
        let bad = || format!("“{item}” is not a YYYY-MM month or YYYY-MM..YYYY-MM range.");
        if let Some((a, b)) = item.split_once("..") {
            let (a, b) = (Month::parse(a).ok_or_else(bad)?, Month::parse(b).ok_or_else(bad)?);
            if a.months_since(b).abs() > 1200 {
                return Err(format!("“{item}” spans more than 100 years."));
            }
            let mut m = a;
            loop {
                push(m);
                if m == b {
                    break;
                }
                m = if a <= b { m.next() } else { m.prev() };
            }
        } else {
            push(Month::parse(item).ok_or_else(bad)?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(s: &str) -> Month {
        Month::parse(s).unwrap()
    }

    fn with_output() -> Settings {
        Settings { output_folder: "out".into(), ..Settings::default() }
    }

    #[test]
    fn default_schedule_runs_backward_from_latest_month() {
        let months = with_output().schedule(m("2026-09")).unwrap();
        assert_eq!(months[0], m("2026-09"));
        assert_eq!(months[1], m("2026-08"));
        assert_eq!(*months.last().unwrap(), CHESSCOM_FIRST_MONTH);
    }

    #[test]
    fn forward_schedule_stops_at_latest_complete_month() {
        let mut s = with_output();
        s.months = MonthPlan {
            start: "2026-06".into(),
            direction: Direction::Forward,
            stop: "2027-01".into(),
            ..MonthPlan::default()
        };
        let months = s.schedule(m("2026-09")).unwrap();
        assert_eq!(months, vec![m("2026-06"), m("2026-07"), m("2026-08"), m("2026-09")]);
    }

    #[test]
    fn backward_schedule_honors_stop() {
        let mut s = with_output();
        s.months = MonthPlan { start: "2026-03".into(), stop: "2025-12".into(), ..MonthPlan::default() };
        assert_eq!(s.schedule(m("2026-09")).unwrap(), vec![m("2026-03"), m("2026-02"), m("2026-01"), m("2025-12")]);
    }

    #[test]
    fn specific_months_and_ranges() {
        assert_eq!(
            parse_month_list("2026-03..2026-01, 2025-11; 2026-02").unwrap(),
            vec![m("2026-03"), m("2026-02"), m("2026-01"), m("2025-11")]
        );
        assert!(parse_month_list("2026-1x").is_err());
        let mut s = with_output();
        s.months = MonthPlan { mode: MonthMode::Specific, specific: "2026-10, 2026-08".into(), ..MonthPlan::default() };
        assert_eq!(s.schedule(m("2026-09")).unwrap(), vec![m("2026-08")]);
    }

    #[test]
    fn validation_normalizes_and_rejects() {
        let mut s = with_output();
        s.floors = vec![2700, 2400, 2700];
        s.seed_titles = vec!["im".into(), "GM".into()];
        let v = s.validated().unwrap();
        assert_eq!(v.floors, vec![2400, 2700]);
        assert_eq!(v.seed_titles, vec!["GM", "IM"]);
        s.request_interval_seconds = 0.2;
        assert_eq!(s.validated().unwrap_err().field, "request_interval_seconds");
        let mut s = with_output();
        s.time_classes = TimeClasses { bullet: false, blitz: false, rapid: false, daily: false };
        assert_eq!(s.validated().unwrap_err().field, "time_classes");
    }

    #[test]
    fn narrowing_detection() {
        let old = with_output();
        let mut new = old.clone();
        new.checkmate_only = true;
        new.floors = vec![2700];
        assert!(new.narrows(&old));
        new.time_classes.bullet = true;
        assert!(!new.narrows(&old));
        let mut new = old.clone();
        new.floors = vec![2300];
        assert!(!new.narrows(&old));
        let mut new = old.clone();
        new.require_identification = true;
        assert!(new.narrows(&old));
        new.rating_source = RatingSource::Fide;
        assert!(!new.narrows(&old));
    }

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{"floors":[2700]}"#).unwrap();
        assert_eq!(s.floors, vec![2700]);
        assert!(s.time_classes.blitz && !s.time_classes.bullet);
    }
}

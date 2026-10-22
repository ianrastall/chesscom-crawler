//! Conversions between domain types and ABI messages.

use chesscom_crawler_core::settings::{
    Direction, MonthMode, MonthPlan, RatingSource, Settings, SettingsError, TimeClasses,
};
use chesscom_crawler_core::status::{Event, Level, RunState, Status};

use crate::messages;

fn month_text(m: Option<chesscom_crawler_core::Month>) -> String {
    m.map(|m| m.to_string()).unwrap_or_default()
}

pub fn settings_to_message(s: &Settings) -> messages::Settings {
    messages::Settings {
        rating_source: match s.rating_source {
            RatingSource::Chesscom => messages::RatingSource::Chesscom,
            RatingSource::Fide => messages::RatingSource::Fide,
        } as i32,
        require_identification: s.require_identification,
        floors: s.floors.iter().map(|&f| u32::from(f)).collect(),
        bullet: s.time_classes.bullet,
        blitz: s.time_classes.blitz,
        rapid: s.time_classes.rapid,
        daily: s.time_classes.daily,
        standard_only: s.standard_only,
        rated_only: s.rated_only,
        checkmate_only: s.checkmate_only,
        exclude_fair_play_closures: s.exclude_fair_play_closures,
        include_untitled_accounts: s.include_untitled_accounts,
        month_mode: match s.months.mode {
            MonthMode::Continuous => messages::MonthMode::Continuous,
            MonthMode::Specific => messages::MonthMode::Specific,
        } as i32,
        start_month: s.months.start.clone(),
        direction: match s.months.direction {
            Direction::Backward => messages::Direction::Backward,
            Direction::Forward => messages::Direction::Forward,
        } as i32,
        stop_month: s.months.stop.clone(),
        specific_months: s.months.specific.clone(),
        follow_new_months: s.follow_new_months,
        seed_titles: s.seed_titles.clone(),
        extra_accounts: s.extra_accounts.clone(),
        discover_opponents: s.discover_opponents,
        fide_folders: s.fide_folders.clone(),
        max_fide_list_age_months: u32::from(s.max_fide_list_age_months),
        request_interval_seconds: s.request_interval_seconds,
        contact: s.contact.clone(),
        prefilter_margin: u32::from(s.prefilter_margin),
        output_folder: s.output_folder.clone(),
        remove_clock_comments: s.remove_clock_comments,
        start_on_launch: s.start_on_launch,
    }
}

fn narrow<T: TryFrom<u32>>(value: u32, field: &'static str) -> Result<T, SettingsError> {
    T::try_from(value).map_err(|_| SettingsError { field, message: format!("{value} is out of range.") })
}

pub fn settings_from_message(m: &messages::Settings) -> Result<Settings, SettingsError> {
    Ok(Settings {
        rating_source: match messages::RatingSource::try_from(m.rating_source).unwrap_or_default() {
            messages::RatingSource::Chesscom => RatingSource::Chesscom,
            messages::RatingSource::Fide => RatingSource::Fide,
        },
        require_identification: m.require_identification,
        floors: m.floors.iter().map(|&f| narrow(f, "floors")).collect::<Result<_, _>>()?,
        time_classes: TimeClasses { bullet: m.bullet, blitz: m.blitz, rapid: m.rapid, daily: m.daily },
        standard_only: m.standard_only,
        rated_only: m.rated_only,
        checkmate_only: m.checkmate_only,
        exclude_fair_play_closures: m.exclude_fair_play_closures,
        include_untitled_accounts: m.include_untitled_accounts,
        months: MonthPlan {
            mode: match messages::MonthMode::try_from(m.month_mode).unwrap_or_default() {
                messages::MonthMode::Continuous => MonthMode::Continuous,
                messages::MonthMode::Specific => MonthMode::Specific,
            },
            start: m.start_month.clone(),
            direction: match messages::Direction::try_from(m.direction).unwrap_or_default() {
                messages::Direction::Backward => Direction::Backward,
                messages::Direction::Forward => Direction::Forward,
            },
            stop: m.stop_month.clone(),
            specific: m.specific_months.clone(),
        },
        follow_new_months: m.follow_new_months,
        seed_titles: m.seed_titles.clone(),
        extra_accounts: m.extra_accounts.clone(),
        discover_opponents: m.discover_opponents,
        fide_folders: m.fide_folders.clone(),
        max_fide_list_age_months: narrow(m.max_fide_list_age_months, "max_fide_list_age_months")?,
        request_interval_seconds: m.request_interval_seconds,
        contact: m.contact.clone(),
        prefilter_margin: narrow(m.prefilter_margin, "prefilter_margin")?,
        output_folder: m.output_folder.clone(),
        remove_clock_comments: m.remove_clock_comments,
        start_on_launch: m.start_on_launch,
    })
}

pub fn status_to_message(s: &Status) -> messages::Status {
    messages::Status {
        state: match s.state {
            RunState::Idle => messages::RunState::Idle,
            RunState::Running => messages::RunState::Running,
            RunState::Waiting => messages::RunState::Waiting,
            RunState::Stopping => messages::RunState::Stopping,
            RunState::Completed => messages::RunState::Completed,
            RunState::Failed => messages::RunState::Failed,
        } as i32,
        phase: s.phase.clone(),
        month: month_text(s.month),
        month_players_done: s.month_players_done,
        month_players_total: s.month_players_total,
        month_games: s.month_games,
        total_games: s.total_games,
        months_done: s.months_done,
        months_total: s.months_total,
        known_players: s.known_players,
        resolved_players: s.resolved_players,
        requests: s.requests,
        last_request_unix: s.last_request_unix,
        waiting_until_unix: s.waiting_until_unix,
        last_error: s.last_error.clone(),
        fide_first: month_text(s.fide_first),
        fide_last: month_text(s.fide_last),
        fide_index_list: month_text(s.fide_index_list),
    }
}

pub fn event_to_message(e: &Event) -> messages::Event {
    messages::Event {
        unix_ms: e.unix_ms,
        level: match e.level {
            Level::Info => messages::EventLevel::Info,
            Level::Warning => messages::EventLevel::Warning,
            Level::Error => messages::EventLevel::Error,
        } as i32,
        message: e.message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let mut s = Settings { output_folder: "out".into(), ..Settings::default() };
        s.months.mode = MonthMode::Specific;
        s.months.specific = "2026-01..2026-03".into();
        s.months.direction = Direction::Forward;
        s.time_classes.bullet = true;
        s.extra_accounts = vec!["hikaru".into()];
        s.rating_source = RatingSource::Fide;
        s.require_identification = true;
        assert_eq!(settings_from_message(&settings_to_message(&s)).unwrap(), s);
        let mut m = settings_to_message(&s);
        m.floors = vec![70_000];
        assert_eq!(settings_from_message(&m).unwrap_err().field, "floors");
    }
}

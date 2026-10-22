//! The crawl: months in the configured order, accounts within a month.
//!
//! Seed accounts come from the selected Chess.com title lists, additional usernames and
//! opponents discovered earlier. Each month, every eligible seed's monthly archive is read once
//! and each game is tested, cheapest gate first:
//!
//! 1. local gates from the game record (variant, time class, rated, checkmate) and, with
//!    Chess.com ratings, the floor against both ratings recorded with the game;
//! 2. when players must be identified: title-list membership, a rating prefilter, then FIDE
//!    identification, which may need one profile request per account (cached);
//! 3. fair-play closures (one cached profile request per account) and, with FIDE ratings, both
//!    players' standard ratings on the month's list against the floor.
//!
//! With FIDE ratings only accounts already rated at the floor on the month's list are read.
//! All gates must pass, so their order changes only how many requests are made.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::chesscom::{self, Client, Game, Profile, Transport};
use crate::control::{Control, Stopped};
use crate::countries;
use crate::fide::{FideCatalog, FideIndex, FideList, FideRatings, ProfileFacts, Resolution};
use crate::gates::{GameFacts, Rejection};
use crate::month::{Month, unix_now};
use crate::output;
use crate::pgn::{self, PlayerTags};
use crate::settings::{RatingSource, Settings, TITLES};
use crate::status::{Level, RunState, Shared};
use crate::store::{Account, SEED_DISCOVERED, SEED_NONE, Store, StoreError, StoredGame};

const TITLE_LIST_REFRESH: i64 = 7 * 86_400;
const RESOLVED_PROFILE_REFRESH: i64 = 180 * 86_400;
const UNRESOLVED_PROFILE_REFRESH: i64 = 60 * 86_400;
const EXPORT_INTERVAL: Duration = Duration::from_secs(15 * 60);
const FAIR_PLAY_CLOSURE: &str = "closed:fair_play_violations";
/// Bump when name matching changes so stored resolutions are recomputed.
const RESOLVER_VERSION: u32 = 2;

#[derive(Debug)]
pub enum Halt {
    Stopped,
    Fatal(String),
}

impl From<Stopped> for Halt {
    fn from(_: Stopped) -> Self {
        Halt::Stopped
    }
}

impl From<StoreError> for Halt {
    fn from(e: StoreError) -> Self {
        Halt::Fatal(e.to_string())
    }
}

impl From<rusqlite::Error> for Halt {
    fn from(e: rusqlite::Error) -> Self {
        Halt::Fatal(format!("FIDE database: {e}"))
    }
}

impl From<output::ExportError> for Halt {
    fn from(e: output::ExportError) -> Self {
        Halt::Fatal(e.to_string())
    }
}

type Step<T> = Result<T, Halt>;

/// Runs a crawl to completion, a stop request or a fatal error, and records the outcome.
pub fn run(
    settings: Settings,
    state_path: PathBuf,
    transport: Box<dyn Transport>,
    control: Arc<Control>,
    shared: Arc<Shared>,
) {
    shared.update(|s| {
        s.state = RunState::Running;
        s.last_error.clear();
        s.phase = "Opening state database".into();
    });
    // One crawler per data folder, across processes, so requests can never run in parallel.
    let lock_path = state_path.with_extension("lock");
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&lock_path);
    let locked = match &lock {
        Ok(f) => f.try_lock().map_err(|_| "Another crawler is already running with this data folder.".to_string()),
        Err(e) => Err(format!("Cannot open {}: {e}", lock_path.display())),
    };
    let result =
        locked.map_err(Halt::Fatal).and_then(|()| Store::open(&state_path).map_err(Halt::from)).and_then(|store| {
            let client = Client::new(
                transport,
                settings.request_interval_seconds,
                &settings.contact,
                control.clone(),
                shared.clone(),
            );
            let mut crawler = Crawler::new(settings, store, client, control, shared.clone());
            crawler.run()
        });
    match result {
        Ok(()) => {
            shared.info("Crawl completed.");
            shared.update(|s| {
                s.state = RunState::Completed;
                s.phase = "Completed".into();
            });
        }
        Err(Halt::Stopped) => {
            shared.info("Crawl stopped. Progress is kept; starting again resumes it.");
            shared.update(|s| {
                s.state = RunState::Idle;
                s.phase = "Stopped".into();
                s.waiting_until_unix = 0;
            });
        }
        Err(Halt::Fatal(message)) => {
            shared.log(Level::Error, format!("Crawl failed: {message}"));
            shared.update(|s| {
                s.state = RunState::Failed;
                s.phase = "Failed".into();
                s.last_error = message;
                s.waiting_until_unix = 0;
            });
        }
    }
}

struct Candidate {
    username: String,
    rating: i32,
}

pub struct Crawler {
    settings: Settings,
    store: Store,
    client: Client,
    control: Arc<Control>,
    shared: Arc<Shared>,
    catalog: FideCatalog,
    index: Option<FideIndex>,
    ratings: FideRatings,
}

impl Crawler {
    pub fn new(settings: Settings, store: Store, client: Client, control: Arc<Control>, shared: Arc<Shared>) -> Self {
        Crawler {
            settings,
            store,
            client,
            control,
            shared,
            catalog: FideCatalog::default(),
            index: None,
            ratings: FideRatings::default(),
        }
    }

    fn phase(&self, text: impl Into<String>) {
        let text = text.into();
        self.shared.update(|s| s.phase = text);
    }

    fn source(&self) -> &'static str {
        self.settings.rating_source.code()
    }

    fn fide_ratings(&self) -> bool {
        self.settings.rating_source == RatingSource::Fide
    }

    fn run(&mut self) -> Step<()> {
        self.check_progress_basis()?;
        self.refresh_collections()?;
        if self.settings.identifies() {
            self.load_fide()?;
        }
        self.refresh_title_lists()?;
        self.refresh_counts()?;

        let latest = Month::previous_utc();
        let schedule = self.settings.schedule(latest).map_err(|e| Halt::Fatal(e.to_string()))?;
        self.shared.update(|s| {
            s.months_total = schedule.len() as u32;
            s.months_done = 0;
        });
        for month in &schedule {
            self.crawl_month(*month)?;
            self.shared.update(|s| s.months_done += 1);
        }
        if !self.settings.follow_new_months {
            return Ok(());
        }
        let mut newest = schedule.iter().copied().max().unwrap_or(latest).max(latest);
        loop {
            self.wait_for_month_after(newest)?;
            let latest = Month::previous_utc();
            while newest < latest {
                newest = newest.next();
                self.crawl_month(newest)?;
            }
        }
    }

    /// Widened gates invalidate recorded progress; narrowed gates do not.
    fn check_progress_basis(&mut self) -> Step<()> {
        let basis: Option<Settings> = self.store.meta("progress_settings")?.and_then(|j| serde_json::from_str(&j).ok());
        let reset = match &basis {
            Some(old) => !self.settings.narrows(old),
            None => true,
        };
        if reset {
            if basis.is_some() {
                self.shared
                    .info("The settings now admit games that earlier passes rejected; months will be checked again.");
            }
            self.store.reset_progress()?;
            let json = serde_json::to_string(&self.settings).unwrap_or_default();
            self.store.set_meta("progress_settings", &json)?;
        }
        Ok(())
    }

    /// Rewrites existing collection files when settings that shape them have changed.
    fn refresh_collections(&mut self) -> Step<()> {
        let s = &self.settings;
        let shape = serde_json::json!({
            "rating_source": s.rating_source,
            "floors": s.floors,
            "time_classes": s.time_classes,
            "standard_only": s.standard_only,
            "rated_only": s.rated_only,
            "checkmate_only": s.checkmate_only,
            "remove_clock_comments": s.remove_clock_comments,
            "output_folder": s.output_folder,
        })
        .to_string();
        if self.store.meta("collection_settings")?.as_deref() == Some(shape.as_str()) {
            return Ok(());
        }
        let months = self.store.months_with_games(self.source())?;
        if !months.is_empty() {
            self.phase("Rewriting collections for the current settings");
            for m in &months {
                self.control.check()?;
                output::export_month(&self.store, &self.settings, *m)?;
            }
            self.shared.info(format!("Rewrote collections for {} months.", months.len()));
        }
        self.store.set_meta("collection_settings", &shape)?;
        Ok(())
    }

    fn load_fide(&mut self) -> Step<()> {
        self.phase("Reading FIDE rating lists");
        let (catalog, warnings) = FideCatalog::scan(&self.settings.fide_folders);
        for w in warnings {
            self.shared.warn(w);
        }
        let Some(latest) = catalog.latest() else {
            return Err(Halt::Fatal(format!(
                "No FIDE standard rating lists were found in {}.",
                self.settings.fide_folders.join("; ")
            )));
        };
        self.control.check()?;
        let list = catalog.get(latest).cloned().ok_or_else(|| Halt::Fatal("FIDE catalog is inconsistent.".into()))?;
        self.phase(format!("Indexing FIDE list {latest}"));
        let index = FideIndex::load(&list)?;
        self.shared.info(format!(
            "FIDE lists {} to {} ({} lists); names indexed from {} ({} players).",
            catalog.earliest().map(|m| m.to_string()).unwrap_or_default(),
            latest,
            catalog.len(),
            latest,
            index.len()
        ));
        self.shared.update(|s| {
            s.fide_first = catalog.earliest();
            s.fide_last = Some(latest);
            s.fide_index_list = Some(latest);
        });
        self.catalog = catalog;
        self.index = Some(index);
        Ok(())
    }

    fn refresh_title_lists(&mut self) -> Step<()> {
        for title in TITLES {
            let fresh = self.store.title_list_fetched_at(title)?.is_some_and(|t| unix_now() - t < TITLE_LIST_REFRESH);
            if fresh {
                continue;
            }
            self.phase(format!("Reading the Chess.com {title} list"));
            match self.client.titled(title)? {
                Some(players) => {
                    let players: Vec<String> = players.into_iter().filter(|p| chesscom::valid_username(p)).collect();
                    self.shared.info(format!("{title} list: {} accounts.", players.len()));
                    self.store.put_title_list(title, &players, false)?;
                }
                None => {
                    self.shared.warn(format!(
                        "The Chess.com {title} list was not available; it will be tried again in a week."
                    ));
                    self.store.put_title_list(title, &[], false)?;
                }
            }
        }
        self.store.set_seed_titles(&self.settings.seed_titles)?;
        self.store.set_extra_accounts(&self.settings.extra_accounts)?;
        Ok(())
    }

    fn refresh_counts(&self) -> Step<()> {
        let (known, resolved) = self.store.account_counts()?;
        let total = self.store.game_count(None, self.source())?;
        let identifies = self.settings.identifies();
        self.shared.update(|s| {
            s.known_players = if identifies { known } else { 0 };
            s.resolved_players = resolved;
            s.total_games = total;
        });
        Ok(())
    }

    fn wait_for_month_after(&mut self, newest: Month) -> Step<()> {
        loop {
            if Month::previous_utc() > newest {
                return Ok(());
            }
            let next = newest.next().next();
            let until = month_start_unix(next) + 6 * 3600;
            self.shared.update(|s| {
                s.state = RunState::Waiting;
                s.phase = format!("Waiting for {} to complete", newest.next());
                s.waiting_until_unix = until;
                s.month = None;
            });
            let wait = (until - unix_now()).clamp(60, 3600) as u64;
            let slept = self.control.sleep(Duration::from_secs(wait));
            self.shared.update(|s| {
                s.state = RunState::Running;
                s.waiting_until_unix = 0;
            });
            slept?;
        }
    }

    fn fide_list_for(&mut self, month: Month) -> Option<FideList> {
        if let Some(list) = self.catalog.list_for(month, self.settings.max_fide_list_age_months) {
            return Some(list.clone());
        }
        // Lists may have been added since the crawl started.
        let (catalog, _) = FideCatalog::scan(&self.settings.fide_folders);
        let list = catalog.list_for(month, self.settings.max_fide_list_age_months).cloned();
        if !catalog.is_empty() {
            self.shared.update(|s| {
                s.fide_first = catalog.earliest();
                s.fide_last = catalog.latest();
            });
            self.catalog = catalog;
        }
        list
    }

    pub fn crawl_month(&mut self, month: Month) -> Step<()> {
        if self.store.month_status(month)?.as_deref() == Some("complete") {
            return Ok(());
        }
        let month_games = self.store.game_count(Some(month), self.source())? as u32;
        self.shared.update(|s| {
            s.month = Some(month);
            s.month_players_done = 0;
            s.month_players_total = 0;
            s.month_games = month_games;
        });
        let list = if self.fide_ratings() {
            let Some(list) = self.fide_list_for(month) else {
                self.shared.warn(format!(
                    "{month}: no FIDE standard list within {} months; skipped until one is available.",
                    self.settings.max_fide_list_age_months
                ));
                self.store.set_month_status(month, "no_fide_list")?;
                return Ok(());
            };
            Some(list)
        } else {
            None
        };

        if self.settings.identifies() {
            let seeds = self.store.crawl_accounts()?;
            let total = seeds.len();
            for (i, account) in seeds.into_iter().enumerate() {
                if i % 25 == 0 {
                    self.phase(format!("{month}: identifying accounts ({i} of {total})"));
                }
                self.ensure_resolved(account)?;
            }
            self.refresh_counts()?;
        }

        let floor = i32::from(self.settings.min_floor());
        let mut queue = Vec::new();
        for account in self.store.crawl_accounts()? {
            if self.settings.exclude_fair_play_closures && account.status.as_deref() == Some(FAIR_PLAY_CLOSURE) {
                continue;
            }
            if self.settings.identifies() && account.resolution.as_deref() != Some("resolved") {
                continue;
            }
            match (&list, account.fide_id) {
                (Some(list), Some(id)) => {
                    if let Some(rating) = self.ratings.rating(list, id)?
                        && rating >= floor
                    {
                        queue.push(Candidate { username: account.username, rating });
                    }
                }
                (Some(_), None) => {}
                (None, _) => queue.push(Candidate { username: account.username, rating: 0 }),
            }
        }
        // Stable: equal ratings keep the title-list order.
        queue.sort_by_key(|c| std::cmp::Reverse(c.rating));
        let mut queued: HashSet<String> = queue.iter().map(|c| c.username.clone()).collect();
        match &list {
            Some(list) => self.shared.info(format!(
                "{month}: {} accounts rated {floor} or more on the {} FIDE list.",
                queue.len(),
                list.date
            )),
            None => self.shared.info(format!("{month}: {} accounts to read.", queue.len())),
        }

        let mut last_export = Instant::now();
        let mut dirty = false;
        let mut i = 0;
        while i < queue.len() {
            let username = queue[i].username.clone();
            self.shared.update(|s| {
                s.month_players_done = i as u32;
                s.month_players_total = queue.len() as u32;
            });
            if !self.store.account_month_done(&username, month)? {
                self.phase(format!("{month}: reading games of {username}"));
                let found = self.crawl_account_month(&username, month, list.as_ref())?;
                dirty |= found.added > 0;
                let mut added = 0;
                for discovered in found.discovered {
                    if queued.insert(discovered.username.clone()) {
                        queue.push(discovered);
                        added += 1;
                    }
                }
                if added > 0 {
                    self.shared.info(format!("Added {added} opponents of {username} to the crawl."));
                }
                let games = self.store.game_count(Some(month), self.source())? as u32;
                self.shared.update(|s| s.month_games = games);
                self.refresh_counts()?;
            }
            if dirty && last_export.elapsed() >= EXPORT_INTERVAL {
                output::export_month(&self.store, &self.settings, month)?;
                last_export = Instant::now();
                dirty = false;
            }
            i += 1;
        }

        self.phase(format!("{month}: writing collections"));
        let written = output::export_month(&self.store, &self.settings, month)?;
        self.store.set_month_status(month, "complete")?;
        let collections: Vec<String> = written.iter().map(|(f, n)| format!("{f}+: {n}")).collect();
        let rejected: Vec<String> =
            self.store.rejection_counts(month)?.iter().map(|(r, n)| format!("{r} {n}")).collect();
        self.shared.info(format!(
            "{month} complete. Games collected — {}. Rejected — {}.",
            collections.join(", "),
            if rejected.is_empty() { "none".into() } else { rejected.join(", ") }
        ));
        self.shared.update(|s| {
            s.month_players_done = queue.len() as u32;
            s.month_players_total = queue.len() as u32;
        });
        Ok(())
    }

    /// Fetches the account's profile when it has never been read or is stale.
    fn ensure_profile(&mut self, mut account: Account) -> Step<Account> {
        let refresh = if account.resolution.as_deref() == Some("resolved") {
            RESOLVED_PROFILE_REFRESH
        } else {
            UNRESOLVED_PROFILE_REFRESH
        };
        let now = unix_now();
        if account.profile_at.is_some_and(|t| now - t < refresh) {
            return Ok(account);
        }
        match self.client.profile(&account.username)? {
            Some(p) => apply_profile(&mut account, &p),
            None => {
                account.name = None;
                account.status = Some("not_found".into());
            }
        }
        account.profile_at = Some(now);
        // Force re-identification against the refreshed profile.
        account.resolved_list = None;
        self.store.put_account(&account)?;
        Ok(account)
    }

    /// Ensures a current profile and resolves the account against the FIDE index.
    fn ensure_resolved(&mut self, account: Account) -> Step<Account> {
        let index = self.index.as_ref().ok_or_else(|| Halt::Fatal("FIDE index not loaded.".into()))?;
        // Re-resolve (without requests) when the FIDE list or the matching rules change.
        let list_date = format!("{}/r{RESOLVER_VERSION}", index.list_date);
        let mut account = self.ensure_profile(account)?;
        if account.resolved_list.as_deref() == Some(list_date.as_str()) {
            return Ok(account);
        }
        let index = self.index.as_ref().ok_or_else(|| Halt::Fatal("FIDE index not loaded.".into()))?;
        let resolution = match self.store.override_for(&account.username)? {
            Some(Some(id)) => match index.by_id(id) {
                Some(p) => Resolution::Resolved(p.clone()),
                None => Resolution::NoMatch,
            },
            Some(None) => Resolution::NoMatch,
            None if account.status.as_deref() == Some("not_found") => Resolution::NoName,
            None => {
                let title =
                    account.title.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| account.list_title.clone());
                let country = account.country.clone().unwrap_or_default();
                index.resolve(&ProfileFacts {
                    name: account.name.as_deref().unwrap_or(""),
                    title: &title,
                    country: &country,
                })
            }
        };
        account.resolution = Some(resolution.code().to_string());
        account.resolved_list = Some(list_date);
        match resolution {
            Resolution::Resolved(p) => {
                account.fide_title = Some(p.display_title().to_string());
                account.fide_id = Some(p.fide_id);
                account.fide_name = Some(p.name);
            }
            _ => {
                account.fide_id = None;
                account.fide_name = None;
                account.fide_title = None;
            }
        }
        self.store.put_account(&account)?;
        Ok(account)
    }

    fn crawl_account_month(&mut self, username: &str, month: Month, list: Option<&FideList>) -> Step<AccountMonth> {
        let mut found = AccountMonth::default();
        let Some(mut me) = self.store.account(username)? else { return Ok(found) };

        // The archive list only gains months as they pass, so a list read after `month` ended
        // already shows whether that month exists.
        if me.archives_at.is_none_or(|t| t < month_start_unix(month.next())) {
            let months = self.client.archives(username)?.unwrap_or_default();
            me.archives = Some(months.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(","));
            me.archives_at = Some(unix_now());
            self.store.put_account(&me)?;
        }
        let has_month = me.archives.as_deref().unwrap_or("").split(',').any(|m| m == month.to_string());
        let games = if has_month { self.client.month_games(username, month)?.unwrap_or_default() } else { Vec::new() };

        let mut accepted: Vec<(Game, AcceptedGame)> = Vec::new();
        let mut rejected: Vec<(String, Rejection)> = Vec::new();
        let floor = i32::from(self.settings.min_floor());
        for game in games {
            if game.key().is_empty() || self.store.game_seen(game.key(), self.source())? {
                continue;
            }
            match self.evaluate(&game, &mut me, list, floor)? {
                Ok(a) => {
                    if let Some(d) = &a.discovered {
                        found.discovered.push(Candidate { username: d.clone(), rating: a.opponent_rating });
                    }
                    accepted.push((game, a));
                }
                Err(r) => rejected.push((game.key().to_string(), r)),
            }
        }

        let source = self.source();
        self.store.atomically(|store| {
            for (game, a) in &accepted {
                let facts = GameFacts::of(game);
                store.put_game(&StoredGame {
                    key: game.key(),
                    source,
                    month,
                    end_time: game.end_time,
                    time_class: &game.time_class,
                    rated: game.rated,
                    standard: facts.standard,
                    checkmate: facts.checkmate,
                    white_fide: a.white_fide,
                    black_fide: a.black_fide,
                    white_elo: a.white_elo,
                    black_elo: a.black_elo,
                    pgn: &a.pgn,
                })?;
            }
            for (key, r) in &rejected {
                store.put_rejected(key, month, r.code())?;
            }
            store.mark_account_month(username, month)
        })?;
        found.added = accepted.len();
        Ok(found)
    }

    /// Applies every gate to one game from `me`'s archive, cheapest first.
    fn evaluate(
        &mut self,
        game: &Game,
        me: &mut Account,
        list: Option<&FideList>,
        floor: i32,
    ) -> Step<Result<AcceptedGame, Rejection>> {
        if let Err(r) = GameFacts::of(game).check(&self.settings) {
            return Ok(Err(r));
        }
        let me_white = game.white.username.eq_ignore_ascii_case(&me.username);
        let opponent = if me_white { &game.black } else { &game.white };
        if !chesscom::valid_username(&opponent.username) {
            return Ok(Err(Rejection::Unresolved));
        }
        if list.is_none() && (game.white.rating < floor || game.black.rating < floor) {
            return Ok(Err(Rejection::BelowFloor));
        }

        let mut opp = self.store.account(&opponent.username)?.unwrap_or_else(|| Account {
            username: opponent.username.to_ascii_lowercase(),
            display: opponent.username.clone(),
            ..Account::default()
        });
        if self.settings.identifies() {
            if opp.resolution.is_none() || opp.profile_at.is_none() {
                if !self.settings.include_untitled_accounts && opp.list_title.is_empty() {
                    return Ok(Err(Rejection::Untitled));
                }
                let margin = i32::from(self.settings.prefilter_margin);
                if list.is_some() && margin > 0 && opponent.rating < floor - margin {
                    return Ok(Err(Rejection::Prefilter));
                }
            }
            opp = self.ensure_resolved(opp)?;
            if !self.settings.include_untitled_accounts && opp.list_title.is_empty() {
                return Ok(Err(Rejection::Untitled));
            }
            if opp.resolution.as_deref() != Some("resolved") {
                return Ok(Err(Rejection::Unresolved));
            }
            if me.fide_id.is_none() || me.fide_id == opp.fide_id {
                return Ok(Err(if me.fide_id.is_none() { Rejection::Unresolved } else { Rejection::SamePlayer }));
            }
        }
        if self.settings.exclude_fair_play_closures {
            *me = self.ensure_profile(me.clone())?;
            opp = self.ensure_profile(opp)?;
            let fair_play = |a: &Account| a.status.as_deref() == Some(FAIR_PLAY_CLOSURE);
            if fair_play(me) || fair_play(&opp) {
                return Ok(Err(Rejection::FairPlay));
            }
        }

        let (my_rating, opp_rating) = match (list, me.fide_id, opp.fide_id) {
            (Some(list), Some(my_id), Some(opp_id)) => {
                let (Some(mine), Some(theirs)) =
                    (self.ratings.rating(list, my_id)?, self.ratings.rating(list, opp_id)?)
                else {
                    return Ok(Err(Rejection::BelowFloor));
                };
                if mine < floor || theirs < floor {
                    return Ok(Err(Rejection::BelowFloor));
                }
                (mine, theirs)
            }
            (Some(_), _, _) => return Ok(Err(Rejection::Unresolved)),
            (None, _, _) => {
                let mine = if me_white { game.white.rating } else { game.black.rating };
                (mine, opponent.rating)
            }
        };

        let discovered = if self.settings.discover_opponents && opp.seed == SEED_NONE {
            opp.seed = SEED_DISCOVERED;
            self.store.put_account(&opp)?;
            Some(opp.username.clone())
        } else {
            None
        };

        let (white, black) = if me_white { (&*me, &opp) } else { (&opp, &*me) };
        let (white_elo, black_elo) = if me_white { (my_rating, opp_rating) } else { (opp_rating, my_rating) };
        let ids = (white.fide_id.unwrap_or(0), black.fide_id.unwrap_or(0));
        let pgn = if list.is_some() {
            pgn::rewrite(
                &game.pgn,
                &player_tags(white, white_elo, &game.white.username),
                &player_tags(black, black_elo, &game.black.username),
            )
        } else {
            pgn::tidy(&game.pgn)
        };
        Ok(Ok(AcceptedGame {
            white_fide: ids.0,
            black_fide: ids.1,
            white_elo,
            black_elo,
            pgn,
            discovered,
            opponent_rating: opp_rating,
        }))
    }
}

#[derive(Default)]
struct AccountMonth {
    added: usize,
    discovered: Vec<Candidate>,
}

struct AcceptedGame {
    white_fide: i64,
    black_fide: i64,
    white_elo: i32,
    black_elo: i32,
    pgn: String,
    discovered: Option<String>,
    opponent_rating: i32,
}

fn player_tags<'a>(a: &'a Account, elo: i32, account: &'a str) -> PlayerTags<'a> {
    PlayerTags {
        name: a.fide_name.as_deref().unwrap_or(""),
        elo,
        title: a.fide_title.as_deref().unwrap_or(""),
        fide_id: a.fide_id.unwrap_or(0),
        account,
    }
}

fn apply_profile(account: &mut Account, p: &Profile) {
    if !p.username.is_empty() {
        account.display = p.username.clone();
    }
    account.name = p.name.clone().map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    account.title = p.title.clone();
    account.country = Some(countries::code_from_url(&p.country).to_string());
    account.status = Some(p.status.clone());
}

fn month_start_unix(m: Month) -> i64 {
    // Days from 1970-01-01 to the first of the month (inverse of civil_from_days).
    let y = i64::from(m.year) - i64::from(m.month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m.month) + 9) % 12;
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400
}

/// Where the core keeps its state database inside the data directory.
pub fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("state.sqlite")
}

#[cfg(test)]
mod tests;

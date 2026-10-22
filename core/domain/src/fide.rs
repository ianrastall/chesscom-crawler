//! Read-only access to the FIDE standard rating lists.
//!
//! Two layouts are understood and may be mixed: yearly databases (`fide-YYYY.sqlite`, with a
//! `rating_lists` table and a `rating_type` column) and per-list databases
//! (`fide-standard-YYYY-MM.sqlite`, with a `meta.list_date` value). Only standard lists are used.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::countries;
use crate::month::Month;
use crate::names::{self, FideName, MatchKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListLayout {
    PerList,
    Yearly,
}

#[derive(Clone, Debug)]
pub struct FideList {
    pub date: Month,
    pub path: PathBuf,
    pub layout: ListLayout,
}

#[derive(Default)]
pub struct FideCatalog {
    lists: BTreeMap<Month, FideList>,
}

fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
}

fn has_table(conn: &Connection, name: &str) -> bool {
    conn.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some()
}

impl FideCatalog {
    /// Scans folders for FIDE databases. Unreadable files are reported, not fatal.
    pub fn scan(folders: &[String]) -> (FideCatalog, Vec<String>) {
        let mut catalog = FideCatalog::default();
        let mut warnings = Vec::new();
        for folder in folders {
            let entries = match std::fs::read_dir(folder) {
                Ok(e) => e,
                Err(e) => {
                    warnings.push(format!("FIDE folder {folder} cannot be read: {e}"));
                    continue;
                }
            };
            let mut paths: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sqlite")))
                .collect();
            paths.sort();
            for path in paths {
                if let Err(e) = catalog.add_file(&path) {
                    warnings.push(format!("{} skipped: {e}", path.display()));
                }
            }
        }
        (catalog, warnings)
    }

    fn add_file(&mut self, path: &Path) -> rusqlite::Result<()> {
        let file_name = path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if file_name.contains("rapid") || file_name.contains("blitz") {
            return Ok(());
        }
        let conn = open_read_only(path)?;
        if !has_table(&conn, "players") {
            return Ok(());
        }
        if has_table(&conn, "rating_lists") {
            let mut stmt =
                conn.prepare("SELECT DISTINCT list_date FROM rating_lists WHERE rating_type = 'standard'")?;
            let dates = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for date in dates.flatten() {
                if let Some(month) = Month::parse(&date) {
                    // A per-list file for the same month is smaller and preferred.
                    self.lists.entry(month).or_insert(FideList {
                        date: month,
                        path: path.to_path_buf(),
                        layout: ListLayout::Yearly,
                    });
                }
            }
        } else if has_table(&conn, "meta") {
            let date: Option<String> =
                conn.query_row("SELECT value FROM meta WHERE key = 'list_date'", [], |r| r.get(0)).optional()?;
            if let Some(month) = date.as_deref().and_then(Month::parse) {
                self.lists
                    .insert(month, FideList { date: month, path: path.to_path_buf(), layout: ListLayout::PerList });
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.lists.is_empty()
    }

    pub fn len(&self) -> usize {
        self.lists.len()
    }

    pub fn earliest(&self) -> Option<Month> {
        self.lists.keys().next().copied()
    }

    pub fn latest(&self) -> Option<Month> {
        self.lists.keys().next_back().copied()
    }

    /// The most recent list published at or before `month`, if it is at most `max_age` months old.
    pub fn list_for(&self, month: Month, max_age: u8) -> Option<&FideList> {
        let (date, list) = self.lists.range(..=month).next_back()?;
        (month.months_since(*date) <= i32::from(max_age)).then_some(list)
    }

    pub fn get(&self, month: Month) -> Option<&FideList> {
        self.lists.get(&month)
    }
}

/// One player on a FIDE list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FidePlayer {
    pub fide_id: i64,
    pub name: String,
    pub fed: String,
    pub title: String,
    pub w_title: String,
}

impl FidePlayer {
    /// The title to show in PGN headers: the open title, else the women's title.
    pub fn display_title(&self) -> &str {
        if self.title.is_empty() { &self.w_title } else { &self.title }
    }

    pub fn is_titled(&self) -> bool {
        !self.title.is_empty() || !self.w_title.is_empty()
    }

    fn holds(&self, title: &str) -> bool {
        self.title.eq_ignore_ascii_case(title) || self.w_title.eq_ignore_ascii_case(title)
    }
}

const FIDE_TITLES: [&str; 8] = ["GM", "IM", "FM", "CM", "WGM", "WIM", "WFM", "WCM"];

/// What is known about a Chess.com account when resolving it.
pub struct ProfileFacts<'a> {
    pub name: &'a str,
    pub title: &'a str,
    pub country: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Resolved(FidePlayer),
    NoName,
    NoMatch,
    TitleMismatch,
    Ambiguous(usize),
}

impl Resolution {
    pub fn code(&self) -> &'static str {
        match self {
            Resolution::Resolved(_) => "resolved",
            Resolution::NoName => "no_name",
            Resolution::NoMatch => "no_match",
            Resolution::TitleMismatch => "title_mismatch",
            Resolution::Ambiguous(_) => "ambiguous",
        }
    }
}

/// Name index over the most recent list, used to identify players.
pub struct FideIndex {
    pub list_date: Month,
    players: Vec<FidePlayer>,
    postings: HashMap<String, Vec<u32>>,
}

impl FideIndex {
    pub fn load(list: &FideList) -> rusqlite::Result<FideIndex> {
        let conn = open_read_only(&list.path)?;
        let sql = match list.layout {
            ListLayout::PerList => {
                "SELECT fide_id, name, fed, title, w_title FROM players WHERE fide_id IS NOT NULL AND name IS NOT NULL"
            }
            ListLayout::Yearly => {
                "SELECT fide_id, name, fed, title, w_title FROM players
                 WHERE fide_id IS NOT NULL AND name IS NOT NULL AND list_date = ?1 AND rating_type = 'standard'"
            }
        };
        let mut stmt = conn.prepare(sql)?;
        let map = |r: &rusqlite::Row| {
            Ok(FidePlayer {
                fide_id: r.get(0)?,
                name: r.get::<_, String>(1)?.trim().to_string(),
                fed: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                title: r.get::<_, Option<String>>(3)?.unwrap_or_default().trim().to_string(),
                w_title: r.get::<_, Option<String>>(4)?.unwrap_or_default().trim().to_string(),
            })
        };
        let rows: Vec<FidePlayer> = match list.layout {
            ListLayout::PerList => stmt.query_map([], map)?.collect::<Result<_, _>>()?,
            ListLayout::Yearly => stmt.query_map([list.date.to_string()], map)?.collect::<Result<_, _>>()?,
        };
        Ok(FideIndex::from_players(list.date, rows))
    }

    pub fn from_players(list_date: Month, players: Vec<FidePlayer>) -> FideIndex {
        let mut postings: HashMap<String, Vec<u32>> = HashMap::new();
        for (i, p) in players.iter().enumerate() {
            let mut toks = names::tokens(&p.name);
            toks.sort_unstable();
            toks.dedup();
            for t in toks {
                postings.entry(t).or_default().push(i as u32);
            }
        }
        FideIndex { list_date, players, postings }
    }

    pub fn len(&self) -> usize {
        self.players.len()
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    pub fn by_id(&self, fide_id: i64) -> Option<&FidePlayer> {
        self.players.iter().find(|p| p.fide_id == fide_id)
    }

    /// Identifies a Chess.com account as one FIDE player.
    ///
    /// The profile name must match exactly one FIDE name, and titles must agree: an account with
    /// a FIDE-type title (verified by Chess.com) must match a FIDE player holding it, and an
    /// untitled or national-title account may only match an untitled FIDE player. This keeps
    /// fan accounts named after a titled player from resolving to that player.
    pub fn resolve(&self, facts: &ProfileFacts) -> Resolution {
        let profile = names::profile_tokens(facts.name);
        if profile.len() < 2 {
            return Resolution::NoName;
        }
        // Every FIDE name sharing a word with the profile; matching checks the surname.
        let mut ids: Vec<u32> = profile.iter().filter_map(|t| self.postings.get(t)).flatten().copied().collect();
        ids.sort_unstable();
        ids.dedup();
        let matches: Vec<(MatchKind, &FidePlayer)> = ids
            .into_iter()
            .map(|i| &self.players[i as usize])
            .filter_map(|p| names::match_kind(&profile, &FideName::parse(&p.name)).map(|k| (k, p)))
            .collect();
        if matches.is_empty() {
            return Resolution::NoMatch;
        }
        let title = facts.title.trim().to_ascii_uppercase();
        let fide_title = FIDE_TITLES.contains(&title.as_str());
        let mut titled: Vec<(MatchKind, &FidePlayer)> =
            matches.into_iter().filter(|(_, p)| if fide_title { p.holds(&title) } else { !p.is_titled() }).collect();
        let Some(best) = titled.iter().map(|(k, _)| *k).max() else { return Resolution::TitleMismatch };
        titled.retain(|(k, _)| *k == best);
        let candidates: Vec<&FidePlayer> = titled.into_iter().map(|(_, p)| p).collect();
        match candidates.len() {
            0 => Resolution::TitleMismatch,
            1 => Resolution::Resolved(candidates[0].clone()),
            n => {
                let fed = countries::fide_federation(facts.country);
                let same_fed: Vec<_> = candidates.iter().filter(|p| fed.is_some_and(|f| p.fed == f)).collect();
                if same_fed.len() == 1 {
                    Resolution::Resolved((*same_fed[0]).clone())
                } else {
                    Resolution::Ambiguous(n)
                }
            }
        }
    }
}

/// Rating lookups against the catalogued lists, keeping a few connections open.
#[derive(Default)]
pub struct FideRatings {
    connections: HashMap<PathBuf, Connection>,
    cache: HashMap<(Month, i64), Option<i32>>,
}

impl FideRatings {
    /// A player's standard rating on `list`; `None` when absent or unrated.
    pub fn rating(&mut self, list: &FideList, fide_id: i64) -> rusqlite::Result<Option<i32>> {
        if let Some(r) = self.cache.get(&(list.date, fide_id)) {
            return Ok(*r);
        }
        if !self.connections.contains_key(&list.path) {
            if self.connections.len() >= 8 {
                self.connections.clear();
            }
            self.connections.insert(list.path.clone(), open_read_only(&list.path)?);
        }
        let conn = &self.connections[&list.path];
        let rating: Option<Option<i32>> = match list.layout {
            ListLayout::PerList => conn
                .query_row("SELECT rating FROM players WHERE fide_id = ?1 LIMIT 1", [fide_id], |r| r.get(0))
                .optional()?,
            ListLayout::Yearly => conn
                .query_row(
                    "SELECT rating FROM players WHERE fide_id = ?1 AND list_date = ?2 AND rating_type = 'standard' LIMIT 1",
                    params![fide_id, list.date.to_string()],
                    |r| r.get(0),
                )
                .optional()?,
        };
        let rating = rating.flatten().filter(|r| *r > 0);
        if self.cache.len() > 200_000 {
            self.cache.clear();
        }
        self.cache.insert((list.date, fide_id), rating);
        Ok(rating)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn player(id: i64, name: &str, fed: &str, title: &str) -> FidePlayer {
        let (title, w_title) = if title.starts_with('W') { ("", title) } else { (title, "") };
        FidePlayer { fide_id: id, name: name.into(), fed: fed.into(), title: title.into(), w_title: w_title.into() }
    }

    fn index() -> FideIndex {
        FideIndex::from_players(
            Month::parse("2026-10").unwrap(),
            vec![
                player(1503014, "Carlsen, Magnus", "NOR", "GM"),
                player(2016192, "Nakamura, Hikaru", "USA", "GM"),
                player(10, "Smith, John", "ENG", "IM"),
                player(11, "Smith, John", "USA", "IM"),
                player(12, "Smith, John", "CAN", "IM"),
                player(20, "Doe, Jane Marie", "USA", "WGM"),
                player(30, "Brown, Alex", "USA", ""),
                player(14120828, "Bortnyk, Olexandr", "UKR", "GM"),
                player(14120836, "Bortnyk, Mykola", "UKR", "IM"),
                player(40, "Carlsen, Magnus Junior", "NOR", ""),
            ],
        )
    }

    fn facts<'a>(name: &'a str, title: &'a str, country: &'a str) -> ProfileFacts<'a> {
        ProfileFacts { name, title, country }
    }

    #[test]
    fn resolves_titled_accounts() {
        let idx = index();
        match idx.resolve(&facts("Magnus Carlsen", "GM", "NO")) {
            Resolution::Resolved(p) => assert_eq!(p.fide_id, 1503014),
            other => panic!("{other:?}"),
        }
        assert!(matches!(idx.resolve(&facts("Jane Doe", "WGM", "US")), Resolution::Resolved(p) if p.fide_id == 20));
        assert!(
            matches!(idx.resolve(&facts("Oleksandr Bortnyk", "GM", "US")), Resolution::Resolved(p) if p.fide_id == 14120828)
        );
    }

    #[test]
    fn rejects_fan_accounts_and_missing_names() {
        let idx = index();
        assert_eq!(idx.resolve(&facts("Magnus Carlsen", "IM", "NO")), Resolution::TitleMismatch);
        assert_eq!(idx.resolve(&facts("Hikaru", "GM", "US")), Resolution::NoName);
        assert_eq!(idx.resolve(&facts("Nobody Known", "GM", "US")), Resolution::NoMatch);
        assert!(matches!(idx.resolve(&facts("Alex Brown", "", "US")), Resolution::Resolved(p) if p.fide_id == 30));
        assert!(matches!(idx.resolve(&facts("Alex Brown", "NM", "US")), Resolution::Resolved(p) if p.fide_id == 30));
    }

    #[test]
    fn federation_breaks_ties() {
        let idx = index();
        assert!(matches!(idx.resolve(&facts("John Smith", "IM", "XE")), Resolution::Resolved(p) if p.fide_id == 10));
        assert_eq!(idx.resolve(&facts("John Smith", "IM", "FR")), Resolution::Ambiguous(3));
    }

    #[test]
    fn catalog_reads_both_layouts() {
        let dir = tempfile::tempdir().unwrap();
        let per = dir.path().join("fide-standard-2026-10.sqlite");
        let c = Connection::open(&per).unwrap();
        c.execute_batch(
            "CREATE TABLE players (list_date TEXT, fide_id INTEGER, name TEXT, fed TEXT, title TEXT, w_title TEXT, rating INTEGER);
             CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);
             INSERT INTO meta VALUES ('list_date', '2026-10');
             INSERT INTO players VALUES ('2026-10', 1503014, 'Carlsen, Magnus', 'NOR', 'GM', NULL, 2823);",
        )
        .unwrap();
        drop(c);
        let yearly = dir.path().join("fide-2018.sqlite");
        let c = Connection::open(&yearly).unwrap();
        c.execute_batch(
            "CREATE TABLE players (list_date TEXT, fide_id INTEGER, name TEXT, fed TEXT, title TEXT, w_title TEXT, rating INTEGER, rating_type TEXT);
             CREATE TABLE rating_lists (source_db TEXT, rating_type TEXT, list_date TEXT);
             INSERT INTO rating_lists VALUES ('a', 'standard', '2018-05'), ('b', 'standard', '2018-06');
             INSERT INTO players VALUES ('2018-06', 1503014, 'Carlsen, Magnus', 'NOR', 'GM', NULL, 2842, 'standard');",
        )
        .unwrap();
        drop(c);

        let (cat, warnings) = FideCatalog::scan(&[dir.path().to_string_lossy().into_owned()]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(cat.len(), 3);
        assert_eq!(cat.latest(), Month::parse("2026-10"));
        let m = |s| Month::parse(s).unwrap();
        assert_eq!(cat.list_for(m("2018-08"), 3).unwrap().date, m("2018-06"));
        assert!(cat.list_for(m("2018-10"), 3).is_none());

        let mut ratings = FideRatings::default();
        assert_eq!(ratings.rating(cat.get(m("2026-10")).unwrap(), 1503014).unwrap(), Some(2823));
        assert_eq!(ratings.rating(cat.get(m("2018-06")).unwrap(), 1503014).unwrap(), Some(2842));
        assert_eq!(ratings.rating(cat.get(m("2018-05")).unwrap(), 1503014).unwrap(), None);

        let idx = FideIndex::load(cat.get(cat.latest().unwrap()).unwrap()).unwrap();
        assert_eq!(idx.len(), 1);
    }
}

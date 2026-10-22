//! Crawl state in SQLite: accounts and their resolutions, per-account month progress, collected
//! games and the keys of rejected games.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::month::{Month, unix_now};

pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS accounts (
    username TEXT PRIMARY KEY,
    display TEXT NOT NULL,
    list_title TEXT NOT NULL DEFAULT '',
    seed INTEGER NOT NULL DEFAULT 0,
    profile_at INTEGER,
    name TEXT,
    title TEXT,
    country TEXT,
    status TEXT,
    resolution TEXT,
    resolved_list TEXT,
    fide_id INTEGER,
    fide_name TEXT,
    fide_title TEXT,
    archives TEXT,
    archives_at INTEGER
);
CREATE INDEX IF NOT EXISTS accounts_fide ON accounts(fide_id);
CREATE TABLE IF NOT EXISTS title_lists (title TEXT PRIMARY KEY, fetched_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS overrides (username TEXT PRIMARY KEY, fide_id INTEGER);
CREATE TABLE IF NOT EXISTS account_months (
    username TEXT NOT NULL,
    month TEXT NOT NULL,
    done_at INTEGER NOT NULL,
    PRIMARY KEY (username, month)
);
CREATE TABLE IF NOT EXISTS games (
    key TEXT NOT NULL,
    source TEXT NOT NULL,
    month TEXT NOT NULL,
    end_time INTEGER NOT NULL,
    time_class TEXT NOT NULL,
    rated INTEGER NOT NULL,
    standard INTEGER NOT NULL,
    checkmate INTEGER NOT NULL,
    white_fide INTEGER NOT NULL,
    black_fide INTEGER NOT NULL,
    white_elo INTEGER NOT NULL,
    black_elo INTEGER NOT NULL,
    pgn TEXT NOT NULL,
    PRIMARY KEY (key, source)
);
CREATE INDEX IF NOT EXISTS games_month ON games(source, month, end_time);
CREATE TABLE IF NOT EXISTS rejected (key TEXT PRIMARY KEY, month TEXT NOT NULL, reason TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS months (month TEXT PRIMARY KEY, status TEXT NOT NULL, updated_at INTEGER NOT NULL);
";

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    NewerSchema(i64),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Sqlite(e) => write!(f, "state database: {e}"),
            StoreError::NewerSchema(v) => {
                write!(f, "state database uses schema {v}, newer than this version supports ({SCHEMA_VERSION})")
            }
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub username: String,
    pub display: String,
    pub list_title: String,
    pub seed: i64,
    pub profile_at: Option<i64>,
    pub name: Option<String>,
    pub title: Option<String>,
    pub country: Option<String>,
    pub status: Option<String>,
    pub resolution: Option<String>,
    pub resolved_list: Option<String>,
    pub fide_id: Option<i64>,
    pub fide_name: Option<String>,
    pub fide_title: Option<String>,
    pub archives: Option<String>,
    pub archives_at: Option<i64>,
}

pub const SEED_NONE: i64 = 0;
pub const SEED_TITLE_LIST: i64 = 1;
pub const SEED_DISCOVERED: i64 = 2;
pub const SEED_EXTRA: i64 = 3;

pub struct StoredGame<'a> {
    pub key: &'a str,
    /// The rating source the game was collected under (`RatingSource::code`).
    pub source: &'a str,
    pub month: Month,
    pub end_time: i64,
    pub time_class: &'a str,
    pub rated: bool,
    pub standard: bool,
    pub checkmate: bool,
    /// FIDE IDs, or 0 when the player was not identified.
    pub white_fide: i64,
    pub black_fide: i64,
    /// The ratings the floors are tested against, from the game's rating source.
    pub white_elo: i32,
    pub black_elo: i32,
    pub pgn: &'a str,
}

#[derive(Clone, Debug)]
pub struct ExportGame {
    pub time_class: String,
    pub rated: bool,
    pub standard: bool,
    pub checkmate: bool,
    pub identified: bool,
    pub white_elo: i32,
    pub black_elo: i32,
    pub pgn: String,
}

pub struct Store {
    conn: Connection,
}

fn account_from_row(r: &rusqlite::Row) -> rusqlite::Result<Account> {
    Ok(Account {
        username: r.get(0)?,
        display: r.get(1)?,
        list_title: r.get(2)?,
        seed: r.get(3)?,
        profile_at: r.get(4)?,
        name: r.get(5)?,
        title: r.get(6)?,
        country: r.get(7)?,
        status: r.get(8)?,
        resolution: r.get(9)?,
        resolved_list: r.get(10)?,
        fide_id: r.get(11)?,
        fide_name: r.get(12)?,
        fide_title: r.get(13)?,
        archives: r.get(14)?,
        archives_at: r.get(15)?,
    })
}

const ACCOUNT_COLUMNS: &str = "username, display, list_title, seed, profile_at, name, title, country, status, resolution, resolved_list, fide_id, fide_name, fide_title, archives, archives_at";

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        Store::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Store::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        let version: Option<String> =
            conn.query_row("SELECT value FROM meta WHERE key = 'schema'", [], |r| r.get(0)).optional()?;
        match version.and_then(|v| v.parse::<i64>().ok()) {
            Some(v) if v > SCHEMA_VERSION => return Err(StoreError::NewerSchema(v)),
            Some(_) => {}
            None => {
                conn.execute("INSERT OR REPLACE INTO meta VALUES ('schema', ?1)", [SCHEMA_VERSION.to_string()])?;
            }
        }
        Ok(Store { conn })
    }

    /// Runs `f` in one transaction, committing only if it succeeds.
    pub fn atomically<T>(&self, f: impl FnOnce(&Store) -> Result<T>) -> Result<T> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        match f(self) {
            Ok(v) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO meta VALUES (?1, ?2)", [key, value])?;
        Ok(())
    }

    // Accounts ---------------------------------------------------------------------------------

    pub fn account(&self, username: &str) -> Result<Option<Account>> {
        let sql = format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE username = ?1");
        Ok(self.conn.query_row(&sql, [username.to_ascii_lowercase()], account_from_row).optional()?)
    }

    pub fn put_account(&self, a: &Account) -> Result<()> {
        self.conn.execute(
            &format!("INSERT OR REPLACE INTO accounts ({ACCOUNT_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)"),
            params![
                a.username.to_ascii_lowercase(),
                a.display,
                a.list_title,
                a.seed,
                a.profile_at,
                a.name,
                a.title,
                a.country,
                a.status,
                a.resolution,
                a.resolved_list,
                a.fide_id,
                a.fide_name,
                a.fide_title,
                a.archives,
                a.archives_at
            ],
        )?;
        Ok(())
    }

    /// Records the accounts on one Chess.com title list. Seeded accounts are marked as such.
    pub fn put_title_list(&mut self, title: &str, usernames: &[String], seed: bool) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO accounts (username, display, list_title, seed) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(username) DO UPDATE SET list_title = excluded.list_title,
                     seed = MAX(seed, excluded.seed)",
            )?;
            for name in usernames {
                insert.execute(params![
                    name.to_ascii_lowercase(),
                    name,
                    title,
                    if seed { SEED_TITLE_LIST } else { SEED_NONE }
                ])?;
            }
            tx.execute("INSERT OR REPLACE INTO title_lists VALUES (?1, ?2)", params![title, unix_now()])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn title_list_fetched_at(&self, title: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT fetched_at FROM title_lists WHERE title = ?1", [title], |r| r.get(0))
            .optional()?)
    }

    pub fn set_seed_titles(&self, titles: &[String]) -> Result<()> {
        self.conn.execute("UPDATE accounts SET seed = ?1 WHERE seed = ?2", params![SEED_NONE, SEED_TITLE_LIST])?;
        for t in titles {
            self.conn.execute(
                "UPDATE accounts SET seed = ?1 WHERE list_title = ?2 AND seed = ?3",
                params![SEED_TITLE_LIST, t, SEED_NONE],
            )?;
        }
        Ok(())
    }

    /// Marks exactly these accounts as additional seeds, adding any that are not yet known.
    pub fn set_extra_accounts(&self, usernames: &[String]) -> Result<()> {
        self.conn.execute("UPDATE accounts SET seed = ?1 WHERE seed = ?2", params![SEED_NONE, SEED_EXTRA])?;
        for name in usernames {
            self.conn.execute(
                "INSERT INTO accounts (username, display, seed) VALUES (?1, ?2, ?3)
                 ON CONFLICT(username) DO UPDATE SET seed = MAX(seed, excluded.seed)",
                params![name.to_ascii_lowercase(), name, SEED_EXTRA],
            )?;
        }
        Ok(())
    }

    /// Accounts that seed the crawl, seeded title accounts first and in title-list order.
    pub fn crawl_accounts(&self) -> Result<Vec<Account>> {
        let sql = format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE seed > 0 ORDER BY seed, username");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], account_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn override_for(&self, username: &str) -> Result<Option<Option<i64>>> {
        Ok(self
            .conn
            .query_row("SELECT fide_id FROM overrides WHERE username = ?1", [username.to_ascii_lowercase()], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// `Some(id)` maps the account to a FIDE ID; `None` prevents it from resolving.
    pub fn set_override(&self, username: &str, fide_id: Option<i64>) -> Result<()> {
        let user = username.to_ascii_lowercase();
        self.conn.execute("INSERT OR REPLACE INTO overrides VALUES (?1, ?2)", params![user, fide_id])?;
        self.conn.execute("UPDATE accounts SET resolved_list = NULL WHERE username = ?1", [user])?;
        Ok(())
    }

    pub fn clear_override(&self, username: &str) -> Result<bool> {
        let user = username.to_ascii_lowercase();
        self.conn.execute("UPDATE accounts SET resolved_list = NULL WHERE username = ?1", [&user])?;
        Ok(self.conn.execute("DELETE FROM overrides WHERE username = ?1", [user])? > 0)
    }

    pub fn overrides(&self) -> Result<Vec<(String, Option<i64>)>> {
        let mut stmt = self.conn.prepare("SELECT username, fide_id FROM overrides ORDER BY username")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn account_counts(&self) -> Result<(u32, u32)> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(resolution = 'resolved'), 0) FROM accounts WHERE seed > 0 OR profile_at IS NOT NULL",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    pub fn resolution_counts(&self) -> Result<Vec<(String, u32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(resolution, 'pending'), COUNT(*) FROM accounts WHERE seed > 0 OR profile_at IS NOT NULL
             GROUP BY 1 ORDER BY 2 DESC",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // Progress ---------------------------------------------------------------------------------

    pub fn account_month_done(&self, username: &str, month: Month) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM account_months WHERE username = ?1 AND month = ?2",
                params![username.to_ascii_lowercase(), month.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn mark_account_month(&self, username: &str, month: Month) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO account_months VALUES (?1, ?2, ?3)",
            params![username.to_ascii_lowercase(), month.to_string(), unix_now()],
        )?;
        Ok(())
    }

    pub fn month_status(&self, month: Month) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT status FROM months WHERE month = ?1", [month.to_string()], |r| r.get(0))
            .optional()?)
    }

    pub fn set_month_status(&self, month: Month, status: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO months VALUES (?1, ?2, ?3)",
            params![month.to_string(), status, unix_now()],
        )?;
        Ok(())
    }

    /// Forgets month progress and rejected-game keys so months are crawled again. Collected games
    /// and account resolutions are kept.
    pub fn reset_progress(&self) -> Result<()> {
        self.conn.execute_batch("DELETE FROM account_months; DELETE FROM rejected; DELETE FROM months;")?;
        Ok(())
    }

    // Games ------------------------------------------------------------------------------------

    pub fn game_seen(&self, key: &str, source: &str) -> Result<bool> {
        let found = self
            .conn
            .query_row(
                "SELECT 1 FROM games WHERE key = ?1 AND source = ?2 UNION ALL SELECT 1 FROM rejected WHERE key = ?1 LIMIT 1",
                [key, source],
                |_| Ok(()),
            )
            .optional()?;
        Ok(found.is_some())
    }

    pub fn put_game(&self, g: &StoredGame) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO games VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                g.key,
                g.source,
                g.month.to_string(),
                g.end_time,
                g.time_class,
                g.rated,
                g.standard,
                g.checkmate,
                g.white_fide,
                g.black_fide,
                g.white_elo,
                g.black_elo,
                g.pgn
            ],
        )?;
        Ok(())
    }

    pub fn put_rejected(&self, key: &str, month: Month, reason: &str) -> Result<()> {
        self.conn
            .execute("INSERT OR IGNORE INTO rejected VALUES (?1, ?2, ?3)", params![key, month.to_string(), reason])?;
        Ok(())
    }

    pub fn games_for_month(&self, month: Month, source: &str) -> Result<Vec<ExportGame>> {
        let mut stmt = self.conn.prepare(
            "SELECT time_class, rated, standard, checkmate, white_fide > 0 AND black_fide > 0, white_elo, black_elo, pgn
             FROM games WHERE month = ?1 AND source = ?2 ORDER BY end_time, key",
        )?;
        let rows = stmt
            .query_map([month.to_string(), source.to_string()], |r| {
                Ok(ExportGame {
                    time_class: r.get(0)?,
                    rated: r.get(1)?,
                    standard: r.get(2)?,
                    checkmate: r.get(3)?,
                    identified: r.get(4)?,
                    white_elo: r.get(5)?,
                    black_elo: r.get(6)?,
                    pgn: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn months_with_games(&self, source: &str) -> Result<Vec<Month>> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT month FROM games WHERE source = ?1 ORDER BY month")?;
        let rows = stmt.query_map([source], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.iter().filter_map(|m| Month::parse(m)).collect())
    }

    pub fn game_count(&self, month: Option<Month>, source: &str) -> Result<u64> {
        let n: i64 = match month {
            Some(m) => self.conn.query_row(
                "SELECT COUNT(*) FROM games WHERE month = ?1 AND source = ?2",
                [m.to_string(), source.to_string()],
                |r| r.get(0),
            )?,
            None => self.conn.query_row("SELECT COUNT(*) FROM games WHERE source = ?1", [source], |r| r.get(0))?,
        };
        Ok(n as u64)
    }

    pub fn rejection_counts(&self, month: Month) -> Result<Vec<(String, u32)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT reason, COUNT(*) FROM rejected WHERE month = ?1 GROUP BY reason ORDER BY 2 DESC")?;
        let rows = stmt
            .query_map([month.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_progress_and_games() {
        let mut s = Store::open_in_memory().unwrap();
        s.put_title_list("GM", &["Hikaru".into(), "MagnusCarlsen".into()], true).unwrap();
        s.put_title_list("NM", &["someone".into()], false).unwrap();
        assert_eq!(s.crawl_accounts().unwrap().len(), 2);
        let mut a = s.account("HIKARU").unwrap().unwrap();
        assert_eq!(a.display, "Hikaru");
        a.fide_id = Some(2016192);
        a.resolution = Some("resolved".into());
        a.profile_at = Some(1);
        s.put_account(&a).unwrap();
        assert_eq!(s.account_counts().unwrap(), (2, 1));

        s.set_seed_titles(&["NM".into()]).unwrap();
        let seeds: Vec<_> = s.crawl_accounts().unwrap().into_iter().map(|a| a.username).collect();
        assert_eq!(seeds, vec!["someone"]);

        let m = Month::parse("2026-09").unwrap();
        assert!(!s.account_month_done("hikaru", m).unwrap());
        s.mark_account_month("Hikaru", m).unwrap();
        assert!(s.account_month_done("hikaru", m).unwrap());

        let g = StoredGame {
            key: "u1",
            source: "fide",
            month: m,
            end_time: 5,
            time_class: "blitz",
            rated: true,
            standard: true,
            checkmate: false,
            white_fide: 1,
            black_fide: 2,
            white_elo: 2500,
            black_elo: 2600,
            pgn: "x",
        };
        s.put_game(&g).unwrap();
        s.put_rejected("u2", m, "time_class").unwrap();
        assert!(
            s.game_seen("u1", "fide").unwrap()
                && s.game_seen("u2", "fide").unwrap()
                && !s.game_seen("u3", "fide").unwrap()
        );
        assert!(!s.game_seen("u1", "chesscom").unwrap());
        assert_eq!(s.games_for_month(m, "fide").unwrap().len(), 1);
        assert!(s.games_for_month(m, "fide").unwrap()[0].identified);
        assert!(s.games_for_month(m, "chesscom").unwrap().is_empty());
        assert_eq!(s.rejection_counts(m).unwrap(), vec![("time_class".to_string(), 1)]);
        s.reset_progress().unwrap();
        assert!(!s.account_month_done("hikaru", m).unwrap() && !s.game_seen("u2", "fide").unwrap());
        assert_eq!(s.game_count(None, "fide").unwrap(), 1);

        s.set_override("Hikaru", None).unwrap();
        assert_eq!(s.override_for("hikaru").unwrap(), Some(None));
        assert!(s.clear_override("hikaru").unwrap());
        assert_eq!(s.override_for("hikaru").unwrap(), None);
    }

    #[test]
    fn rejects_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sqlite");
        let s = Store::open(&path).unwrap();
        s.set_meta("schema", "99").unwrap();
        drop(s);
        assert!(matches!(Store::open(&path), Err(StoreError::NewerSchema(99))));
    }
}

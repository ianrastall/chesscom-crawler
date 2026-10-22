//! Writes collection PGN files: one folder per rating floor, one file per month.
//!
//! Files are regenerated from the state database, so they always reflect the current gates and
//! floors for the games collected so far. Each file is written to a temporary name and then
//! moved over the previous version.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::gates::GameFacts;
use crate::month::Month;
use crate::pgn;
use crate::settings::{RatingSource, Settings};
use crate::store::Store;

/// `chesscom-2400` for Chess.com ratings, `chesscom-fide-2400` for FIDE ratings.
pub fn collection_name(source: RatingSource, floor: u16) -> String {
    match source {
        RatingSource::Chesscom => format!("chesscom-{floor}"),
        RatingSource::Fide => format!("chesscom-fide-{floor}"),
    }
}

pub fn month_file(output: &Path, source: RatingSource, floor: u16, month: Month) -> PathBuf {
    let name = collection_name(source, floor);
    output.join(&name).join(format!("{name}-{month}.pgn"))
}

#[derive(Debug)]
pub enum ExportError {
    Store(crate::store::StoreError),
    Io(PathBuf, io::Error),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Store(e) => write!(f, "{e}"),
            ExportError::Io(p, e) => write!(f, "cannot write {}: {e}", p.display()),
        }
    }
}

/// Rewrites every collection file for `month`. Returns (floor, games written) pairs.
pub fn export_month(store: &Store, settings: &Settings, month: Month) -> Result<Vec<(u16, usize)>, ExportError> {
    let games = store.games_for_month(month, settings.rating_source.code()).map_err(ExportError::Store)?;
    let output = Path::new(&settings.output_folder);
    let mut written = Vec::new();
    for &floor in &settings.floors {
        let mut text = String::new();
        let mut count = 0;
        for g in &games {
            let facts =
                GameFacts { time_class: &g.time_class, rated: g.rated, standard: g.standard, checkmate: g.checkmate };
            if facts.check(settings).is_err() || g.white_elo < i32::from(floor) || g.black_elo < i32::from(floor) {
                continue;
            }
            if count > 0 {
                text.push('\n');
            }
            if settings.remove_clock_comments {
                match g.pgn.split_once("\n\n") {
                    Some((tags, moves)) => {
                        text.push_str(tags);
                        text.push_str("\n\n");
                        text.push_str(&pgn::strip_clocks(moves));
                    }
                    None => text.push_str(&g.pgn),
                }
            } else {
                text.push_str(&g.pgn);
            }
            count += 1;
        }
        let path = month_file(output, settings.rating_source, floor, month);
        if count == 0 {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(ExportError::Io(path, e)),
            }
        } else {
            write_replacing(&path, text.as_bytes()).map_err(|e| ExportError::Io(path.clone(), e))?;
        }
        written.push((floor, count));
    }
    Ok(written)
}

fn write_replacing(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = path.with_extension("pgn.partial");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    // On Windows, rename replaces an existing destination (MoveFileEx with REPLACE_EXISTING).
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoredGame;

    #[test]
    fn writes_collections_by_floor_and_gate() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let m = Month::parse("2026-09").unwrap();
        let game = |key: &'static str, end, elo, checkmate, pgn: &'static str| StoredGame {
            key,
            source: "fide",
            month: m,
            end_time: end,
            time_class: "blitz",
            rated: true,
            standard: true,
            checkmate,
            white_fide: 1,
            black_fide: 2,
            white_elo: elo,
            black_elo: 2750,
            pgn,
        };
        store.put_game(&game("b", 2, 2710, true, "[Event \"B\"]\n\n1. e4 {[%clk 0:01:00]} 1-0\n")).unwrap();
        store.put_game(&game("a", 1, 2450, false, "[Event \"A\"]\n\n1. d4 1-0\n")).unwrap();
        let mut settings = Settings {
            rating_source: RatingSource::Fide,
            floors: vec![2400, 2700],
            output_folder: dir.path().to_string_lossy().into(),
            ..Settings::default()
        };
        let file = |floor| month_file(dir.path(), RatingSource::Fide, floor, m);

        assert_eq!(export_month(&store, &settings, m).unwrap(), vec![(2400, 2), (2700, 1)]);
        let all = fs::read_to_string(file(2400)).unwrap();
        assert_eq!(all, "[Event \"A\"]\n\n1. d4 1-0\n\n[Event \"B\"]\n\n1. e4 {[%clk 0:01:00]} 1-0\n");

        settings.checkmate_only = true;
        settings.remove_clock_comments = true;
        settings.floors = vec![2400];
        assert_eq!(export_month(&store, &settings, m).unwrap(), vec![(2400, 1)]);
        assert_eq!(fs::read_to_string(file(2400)).unwrap(), "[Event \"B\"]\n\n1. e4 1-0\n");

        settings.time_classes.blitz = false;
        assert_eq!(export_month(&store, &settings, m).unwrap(), vec![(2400, 0)]);
        assert!(!file(2400).exists());
        assert!(dir.path().join("chesscom-fide-2400").exists());
    }
}

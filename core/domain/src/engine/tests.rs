use super::*;
use crate::chesscom::API_ROOT;
use crate::chesscom::tests::FakeTransport;
use crate::settings::MonthMode;
use crate::settings::MonthPlan;
use crate::settings::RatingSource;
use rusqlite::Connection;

fn fide_list(dir: &Path) {
    let c = Connection::open(dir.join("fide-standard-2026-09.sqlite")).unwrap();
    c.execute_batch(
        "CREATE TABLE players (list_date TEXT, fide_id INTEGER, name TEXT, fed TEXT, title TEXT, w_title TEXT, rating INTEGER);
         CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);
         INSERT INTO meta VALUES ('list_date', '2026-09');
         INSERT INTO players VALUES
           ('2026-09', 1503014, 'Carlsen, Magnus', 'NOR', 'GM', NULL, 2830),
           ('2026-09', 2016192, 'Nakamura, Hikaru', 'USA', 'GM', NULL, 2790),
           ('2026-09', 300, 'Bro, Michal', 'CZE', 'IM', NULL, 2450),
           ('2026-09', 400, 'Doe, Jane', 'USA', NULL, 'WGM', 2350);",
    )
    .unwrap();
}

fn profile(username: &str, name: &str, title: &str, country: &str) -> String {
    format!(
        r#"{{"username":"{username}","name":"{name}","title":"{title}","country":"https://api.chess.com/pub/country/{country}","status":"premium"}}"#
    )
}

fn game(uuid: &str, white: (&str, i32, &str), black: (&str, i32, &str), time_class: &str) -> String {
    let pgn = format!(
        "[Event \\\"Live Chess\\\"]\\n[Site \\\"Chess.com\\\"]\\n[Date \\\"2026.09.08\\\"]\\n[Round \\\"-\\\"]\\n[White \\\"{}\\\"]\\n[Black \\\"{}\\\"]\\n[Result \\\"1-0\\\"]\\n[WhiteElo \\\"{}\\\"]\\n[BlackElo \\\"{}\\\"]\\n[Termination \\\"{} won\\\"]\\n\\n1. e4 {{[%clk 0:02:59]}} 1-0\\n",
        white.0, black.0, white.1, black.1, white.0
    );
    format!(
        r#"{{"url":"https://www.chess.com/game/live/{uuid}","pgn":"{pgn}","end_time":1788880073,"rated":true,"uuid":"{uuid}",
            "initial_setup":"{START}","time_class":"{time_class}","rules":"chess",
            "white":{{"rating":{},"result":"{}","username":"{}"}},"black":{{"rating":{},"result":"{}","username":"{}"}}}}"#,
        white.1,
        white.2,
        white.0,
        black.1,
        black.2,
        black.0,
        START = crate::gates::START_FEN
    )
}

fn games(list: &[String]) -> String {
    format!(r#"{{"games":[{}]}}"#, list.join(","))
}

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, FakeTransport) {
    let dir = tempfile::tempdir().unwrap();
    let fide_dir = dir.path().join("fide");
    std::fs::create_dir_all(&fide_dir).unwrap();
    fide_list(&fide_dir);
    let out = dir.path().join("out");

    let fake = FakeTransport::default();
    let r = |path: &str, body: &str| fake.route(&format!("{API_ROOT}/{path}"), 200, body);
    r("titled/GM", r#"{"players":["MagnusCarlsen","Hikaru"]}"#);
    r("titled/IM", r#"{"players":["michobr"]}"#);
    r("titled/WGM", r#"{"players":["janedoe"]}"#);
    r("player/magnuscarlsen", &profile("MagnusCarlsen", "Magnus Carlsen", "GM", "NO"));
    r("player/hikaru", &profile("Hikaru", "Hikaru Nakamura", "GM", "US"));
    r("player/michobr", &profile("michobr", "Michal Bro", "IM", "CZ"));
    r("player/janedoe", &profile("janedoe", "Jane Doe", "WGM", "US"));
    for who in ["magnuscarlsen", "hikaru", "michobr"] {
        r(
            &format!("player/{who}/games/archives"),
            &format!(r#"{{"archives":["{API_ROOT}/player/{who}/games/2026/09"]}}"#),
        );
    }
    let u1 = game("u1", ("MagnusCarlsen", 3300, "win"), ("Hikaru", 3350, "checkmated"), "blitz");
    let u3 = game("u3", ("michobr", 2900, "resigned"), ("MagnusCarlsen", 3300, "win"), "rapid");
    let u6 = game("u6", ("Hikaru", 3350, "win"), ("michobr", 2900, "resigned"), "daily");
    r(
        "player/magnuscarlsen/games/2026/09",
        &games(&[
            u1.clone(),
            game("u2", ("MagnusCarlsen", 3300, "win"), ("Hikaru", 3350, "resigned"), "bullet"),
            u3.clone(),
            game("u4", ("MagnusCarlsen", 3300, "win"), ("randomfan", 2900, "resigned"), "blitz"),
            game("u5", ("janedoe", 2600, "resigned"), ("MagnusCarlsen", 3300, "win"), "blitz"),
        ]),
    );
    r("player/hikaru/games/2026/09", &games(&[u1, u6.clone()]));
    r("player/michobr/games/2026/09", &games(&[u3, u6]));
    (dir, fide_dir, out, fake)
}

#[test]
fn crawls_a_month_with_fide_ratings() {
    let (dir, fide_dir, out, fake) = fixture();
    let settings = Settings {
        rating_source: RatingSource::Fide,
        floors: vec![2400, 2700],
        months: MonthPlan { mode: MonthMode::Specific, specific: "2026-09".into(), ..MonthPlan::default() },
        follow_new_months: false,
        seed_titles: vec!["GM".into()],
        fide_folders: vec![fide_dir.to_string_lossy().into_owned()],
        request_interval_seconds: 0.0,
        output_folder: out.to_string_lossy().into_owned(),
        ..Settings::default()
    };
    let shared = Arc::new(Shared::default());
    run(
        settings.clone(),
        dir.path().join("state.sqlite"),
        Box::new(fake.clone()),
        Arc::new(Control::default()),
        shared.clone(),
    );

    let status = shared.snapshot();
    let events: Vec<String> = shared.drain_events(100).into_iter().map(|e| e.message).collect();
    assert_eq!(status.state, RunState::Completed, "{status:?}\n{events:#?}");
    assert_eq!(status.total_games, 3, "{events:#?}");

    let m = Month::parse("2026-09").unwrap();
    let all = std::fs::read_to_string(output::month_file(&out, RatingSource::Fide, 2400, m)).unwrap();
    assert_eq!(all.matches("[Event ").count(), 3);
    assert!(all.contains("[White \"Carlsen, Magnus\"]\n[Black \"Nakamura, Hikaru\"]"));
    assert!(all.contains("[WhiteElo \"2830\"]\n[BlackElo \"2790\"]"));
    assert!(all.contains("[Black \"Bro, Michal\"]") && all.contains("[BlackFideId \"300\"]"));
    assert!(!all.contains("3300"));
    let elite = std::fs::read_to_string(output::month_file(&out, RatingSource::Fide, 2700, m)).unwrap();
    assert_eq!(elite.matches("[Event ").count(), 1);

    let requested = fake.requested();
    assert!(!requested.iter().any(|u| u.contains("randomfan")), "untitled accounts are never requested");
    assert_eq!(requested.iter().filter(|u| u.ends_with("player/hikaru")).count(), 1, "profiles are cached");
    assert!(events.iter().any(|e| e.contains("Added 1 opponents of magnuscarlsen")), "{events:#?}");

    let store = Store::open(&dir.path().join("state.sqlite")).unwrap();
    let reasons = store.rejection_counts(m).unwrap();
    for reason in ["time_class", "untitled", "below_floor"] {
        assert!(reasons.iter().any(|(r, _)| r == reason), "{reasons:?}");
    }
    assert_eq!(store.month_status(m).unwrap().as_deref(), Some("complete"));
    drop(store);

    // A second pass makes no archive or game requests for the completed month.
    let before = fake.requested().len();
    let shared = Arc::new(Shared::default());
    run(
        settings,
        dir.path().join("state.sqlite"),
        Box::new(fake.clone()),
        Arc::new(Control::default()),
        shared.clone(),
    );
    assert_eq!(shared.snapshot().state, RunState::Completed);
    assert_eq!(fake.requested().len(), before);
}

#[test]
fn crawls_a_month_with_chesscom_ratings() {
    let (dir, _fide_dir, out, fake) = fixture();
    let settings = Settings {
        floors: vec![2700],
        months: MonthPlan { mode: MonthMode::Specific, specific: "2026-09".into(), ..MonthPlan::default() },
        follow_new_months: false,
        seed_titles: vec!["GM".into()],
        fide_folders: vec![],
        request_interval_seconds: 0.0,
        output_folder: out.to_string_lossy().into_owned(),
        ..Settings::default()
    };
    assert_eq!(settings.rating_source, RatingSource::Chesscom);
    let shared = Arc::new(Shared::default());
    run(
        settings,
        dir.path().join("state.sqlite"),
        Box::new(fake.clone()),
        Arc::new(Control::default()),
        shared.clone(),
    );
    let status = shared.snapshot();
    let events: Vec<String> = shared.drain_events(100).into_iter().map(|e| e.message).collect();
    assert_eq!(
        status.state,
        RunState::Completed,
        "{status:?}
{events:#?}"
    );

    // u1, u3, u4 (an untitled opponent is fine without identification) and u6; u2 is bullet
    // and u5 is below the floor on Chess.com ratings.
    let m = Month::parse("2026-09").unwrap();
    let all = std::fs::read_to_string(output::month_file(&out, RatingSource::Chesscom, 2700, m)).unwrap();
    assert_eq!(all.matches("[Event ").count(), 4, "{all}");
    assert!(all.contains("[White \"MagnusCarlsen\"]") && all.contains("[WhiteElo \"3300\"]"));
    assert!(!all.contains("FideId") && !all.contains("CurrentPosition"));
    assert!(out.join("chesscom-2700").exists());

    // No identification: profiles are read only for the fair-play check of accepted games.
    let requested = fake.requested();
    assert!(!requested.iter().any(|u| u.ends_with("player/janedoe")), "{requested:#?}");
    assert!(events.iter().any(|e| e.contains("opponents of magnuscarlsen")), "{events:#?}");
}

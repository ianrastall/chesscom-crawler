# Chess.com Crawler

A Windows notification-area app that collects Chess.com games into monthly PGN collections through the Chess.com published-data API. Part of the Jupiter family.

- **Two kinds of collection.** By default games keep their Chess.com usernames and ratings, and both players' ratings recorded with the game must meet the floor. Optionally, players are identified against the local FIDE rating lists: games then carry real names, FIDE standard ratings, titles and FIDE IDs, and both players' FIDE ratings must meet the floor. That collection is much smaller.
- **Several floors at once.** Floors such as 2400 and 2700 each write their own collection from one crawl.
- **Filters.** Time controls (bullet off by default), standard chess only, rated only, checkmate only, accounts closed for fair-play violations. With identification, accounts without a matching real name, including bots, are excluded.
- **Months.** By default the crawl starts with the previous month and works backwards, then collects each new month once it completes. A start month, direction, stop month or a list of specific months and ranges can be chosen instead.
- **Polite.** One request at a time, 3 seconds apart by default (never less than 1). Rate-limit and server errors pause from one minute to 30 minutes, honouring `Retry-After`. Profiles, title lists and archive lists are cached; finished months are never requested again. Only one crawler can run per data folder.

## Use

Run `ChesscomCrawler.exe`. Closing the window keeps it in the notification area; its menu has Open, Start/Stop crawl and Exit. Settings has an option to start when you sign in to Windows.

Collections are written to `Documents\Chess.com collections` by default:

```text
chesscom-2400\chesscom-2400-2026-09.pgn            Chess.com ratings
chesscom-fide-2700\chesscom-fide-2700-2026-09.pgn  FIDE ratings and real names
```

Settings and crawl state live in `%LOCALAPPDATA%\Jupiter\ChesscomCrawler` (`settings.json`, `state.sqlite`, `ui.json`).

The same folder contains `chesscom-crawler.exe`, a command-line shell over the same core:

```text
chesscom-crawler run                      crawl in the foreground
chesscom-crawler status                   counts by identification result and month
chesscom-crawler resolve "Magnus Carlsen" --title GM --country NO
chesscom-crawler override set <username> <fide-id> | block <username> | clear <username> | list
chesscom-crawler export                   rewrite all collection files
chesscom-crawler reset-progress           crawl finished months again
```

## Build

Requirements: Rust 1.99.1 (pinned in `rust-toolchain.toml`), .NET SDK 10.0.1xx (`global.json`), Visual Studio Build Tools with MSVC and the Windows 11 SDK, Windows 11 x64.

```powershell
pwsh build/build.ps1
```

This formats, lints and tests the Rust workspace, runs the C# interop checks and publishes a self-contained portable folder and zip to `dist/`. See [HANDOFF.md](HANDOFF.md) for architecture, data formats and known limitations, and [docs/decisions.md](docs/decisions.md) for the design choices.

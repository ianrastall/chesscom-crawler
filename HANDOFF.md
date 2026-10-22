# Handoff

Repository: `D:\jupiter\chesscom-crawler` (Git). Version 0.1.0. Decisions: [docs/decisions.md](docs/decisions.md).

## Layout

```text
proto/crawler.proto                  ABI message schema (generated on both sides at build time)
core/domain/                         Rust domain crate: settings, months, API client, gates, FIDE, store, output, engine
core/ffi/                            cdylib C ABI facade (chesscom_crawler_ffi.dll)
cli/                                 chesscom-crawler.exe
src/ChesscomCrawler.Interop/         LibraryImport declarations, SafeHandles, CrawlerCore service
src/ChesscomCrawler.App/             WinUI 3 app: tray icon, Status and Settings views, accent, sign-in entry
tests/ChesscomCrawler.InteropTests/  console checks across the real C# to DLL boundary
build/build.ps1                      checks, tests and portable publish; build/native.targets runs Cargo from MSBuild
build/make_icon.py                   regenerates Assets/app.ico (Pillow)
```

## Commands

```powershell
cargo test --workspace --locked --target-dir target
cargo clippy --workspace --all-targets --locked --target-dir target -- -D warnings
dotnet run --project tests/ChesscomCrawler.InteropTests -c Release
dotnet build src/ChesscomCrawler.App -c Debug     # also builds the Rust DLL
pwsh build/build.ps1                               # everything, then dist/ChesscomCrawler and the zip
```

Always pass `--target-dir target`: this machine sets `CARGO_TARGET_DIR=D:\dev\lang\rust\target`, and the MSBuild integration expects the DLL under the repository's `target\`. Close the app before rebuilding; Windows locks the loaded DLL. Set `CHESSCOM_CRAWLER_DATA_DIR` to run the app against a scratch data folder.

## Toolchain (resolved 22 October 2026)

Rust 1.99.1 (x86_64-pc-windows-msvc only); .NET SDK 10.0.113; Windows App SDK 2.5.1; Microsoft.Windows.SDK.BuildTools 10.0.28000.2705; CommunityToolkit.Mvvm 8.4.2; Google.Protobuf 3.36.2; Grpc.Tools 2.84.0; prost/prost-build 0.14.4 with protox 0.10.0; ureq 3.5.0; rusqlite 0.40.2 (bundled SQLite); clap 4.6.7. The Windows App SDK 2.0 family sets the upgrade schedule (supported to April 2027); the owner is Ian.

## Data

| File | Owner | Notes |
| --- | --- | --- |
| `%LOCALAPPDATA%\Jupiter\ChesscomCrawler\settings.json` | Rust | `{version: 1, settings}`; a newer version or unreadable file is left untouched and defaults are used |
| `state.sqlite` | Rust | schema 1, WAL; accounts and identifications, title lists, overrides, account-month progress, collected games (per rating source), rejected-game keys, month status; a newer schema is refused |
| `state.lock` | Rust | held while a crawl runs |
| `ui.json` | C# | accent choice, window bounds |
| `HKCU\...\Run\Jupiter.ChesscomCrawler` | C# | only when "Start when I sign in" is on; launches with `--background` |
| Collections | Rust | `<output>\chesscom-<floor>\…` or `<output>\chesscom-fide-<floor>\…`, rewritten atomically per month |

## Tested

- 29 Rust domain tests (months, settings, name matching, FIDE catalogue and both list layouts, store, gates, PGN rewriting, output, polite client with back-off and stop, two end-to-end crawls against canned API responses, one per rating source), 3 ABI tests including malformed input.
- C# interop checks: three create/dispose rounds, settings validation errors with field names, export, 400 concurrent calls.
- Live API, FIDE mode via the CLI: Hikaru, September 2026, 2700 floor, 3 s interval: 7 games, real names and FIDE ratings correct, no back-offs.
- Live API, Chess.com mode from the GUI (results in the session notes of 22 October 2026).
- GUI: launch, Status and Settings views, Start and Stop through UI Automation, Windows accent applied.

## Known limitations and open items

- First run with identification reads every seed's profile once (about 9,600 accounts for GM, WGM, IM and FM: about 8 hours at 3 s). Without identification no profile requests are needed except for the fair-play check of players in collected games.
- Chess.com mode reads every seed account every month; at 3 s a month of about 10,000 accounts takes about 8 to 9 hours. Months are independent, so stopping and resuming loses at most one account-month.
- Discovery in Chess.com mode can grow the crawl to every account that has played a game at the floor; this is the "full database" behaviour and is what discovery is for.
- Bots: the public API has no bot flag. They are excluded only through identification.
- Not yet verified: high-contrast themes, text scaling, the folder picker on this machine, the published zip on a clean machine, custom-accent refresh of every control.
- No code signing, no installer, no update mechanism.

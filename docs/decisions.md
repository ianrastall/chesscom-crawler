# Design decisions

Recorded 22 October 2026 against the [Jupiter stack reference](../../_standards/stack-reference_winui3-rust-core.md) and [style draft](../../_standards/jupiter-style-and-interface_draft.md).

## Scope and stack

- **Shell:** WinUI 3 desktop app (notification-area utility) plus a Rust CLI over the same core. Rust owns the crawl, gates, FIDE identification, persistence and PGN output; C# owns windows, the notification icon, appearance and the sign-in entry.
- **Boundary:** C ABI version 1 (`core/ffi`) carrying protobuf messages from `proto/crawler.proto`. Seven exports: `ccc_abi_version`, `ccc_create`, `ccc_call`, `ccc_poll_events`, `ccc_result_bytes`, `ccc_result_free`, `ccc_destroy`. Every command returns quickly; the crawl runs on a Rust thread and reports through polled events and a status snapshot. No callbacks. There is no `core_cancel`: the Stop command is the cancellation, honoured between requests and during waits.
- **Minimum Windows:** Windows 11 24H2 (10.0.26100), the oldest release still serviced for Home and Pro as of October 2026.
- **Deployment:** unpackaged, self-contained portable folder (.NET and Windows App SDK runtimes included), JIT. No MSIX, no signing yet.
- **Exception:** no `tracing` crate. Diagnostics are domain events in a bounded queue that the shells display (activity list, CLI stderr); there is no log file.
- **Exception:** WinUI 3 has no notification-area control, so `Services/TrayIcon.cs` uses `Shell_NotifyIconW` with a hidden top-level window (not message-only, so `TaskbarCreated` and menu dismissal work).
- **Layout:** focused utility with two views chosen by a `SelectorBar` (Status, Settings). No menu bar, as the style draft allows for small single-task utilities.

## Crawl

- **Discovery of games:** the API has no search by rating, so accounts are crawled. Seeds are the selected Chess.com title lists plus additional usernames; opponents whose games pass every gate can be added to the crawl. Each game appears in both players' archives and is de-duplicated by UUID.
- **Months:** only complete months (the current month is never crawled). An account's archive list is read once after a month ends, which settles whether that month exists for it; a finished account-month and a finished month are recorded and never requested again.
- **Serial requests:** one `ureq` agent with one connection, a minimum interval between request starts (default 3 s, minimum 1 s), exponential back-off from 60 s to 30 min on 429, 403, 5xx and network errors, honouring `Retry-After`. A file lock (`state.lock`) prevents two crawlers (app and CLI) from running against one data folder.
- **User-Agent:** `JupiterChesscomCrawler/<version> (serial requests)`, plus optional contact details from settings. No contact is sent unless entered.
- **Progress validity:** settings are compared with those under which progress was recorded. Narrower gates keep progress (output is re-filtered); wider gates or a change of rating source clear month progress and rejected-game keys so months are read again. Collected games are always kept.

## Two rating sources

Ian's direction (22 October 2026): FIDE identification is optional, and the default is the full Chess.com-rated database.

- **Chess.com ratings (default):** the floor tests the ratings recorded with the game, before any request. Usernames and ratings stay as Chess.com wrote them; noise tags (`CurrentPosition`, `ECOUrl`, `Timezone`, `StartTime`, `EndDate`, `EndTime`) are removed. *Require identification* adds the FIDE gate and `WhiteFideId`/`BlackFideId` tags without changing names or ratings.
- **FIDE ratings:** each game uses the newest FIDE standard list at or before its month (at most three months older by default). Only seeds rated at the floor on that list are crawled, highest first. PGN tags become FIDE name, Elo, title and FIDE ID; Chess.com usernames are kept as `WhiteAccount`/`BlackAccount`.
- **Gate order:** all gates must pass, so order changes only the request count. Free gates come first (variant, time class, rated, checkmate, Chess.com-rating floor), then title-list membership and a Chess.com rating prefilter, then identification (one cached profile request), then fair play and FIDE ratings.
- **Only standard ratings:** "standard" here means the FIDE standard list; rapid and blitz lists are ignored. Variants and non-standard starting positions are "non-standard games" for the filter.

## FIDE identification

- **Sources:** yearly databases (`D:\dev\proj\fide\fide-YYYY.sqlite`, currently to 2018-06) and per-list databases (`D:\fide-archive\sqlite\fide-standard-YYYY-MM.sqlite`, currently to 2026-10), read-only and in any mix. A per-list file wins over a yearly file for the same month.
- **Name index:** names come from the newest list, about 570,000 players, indexed in about 1.3 s.
- **Matching tiers:** exact (same words in any order), partial (all surname words and the first given name, nothing extra), loose (first surname word, first given name allowing initials, truncation and near spellings, and at most one unaccounted word in names of three or more words). The best tier wins. German transliterations (`ö`/`oe`) fold together.
- **Titles guard identity:** a FIDE-type title on Chess.com (verified by Chess.com) must be held by the FIDE player; an untitled or national-title account may only match an untitled FIDE player. This stops fan accounts named after titled players. A remaining tie is broken by federation; otherwise the account stays unidentified.
- **Measured on live data (Hikaru, September 2026):** 98 of 111 accounts matched before the loose tier; the six name mismatches then all matched correctly; the remaining seven profiles have no name.
- **Overrides:** `chesscom-crawler override` maps an account to a FIDE ID or blocks it. Resolutions are recomputed locally, with no requests, when the newest list or `RESOLVER_VERSION` changes.

## Output

One folder per floor and rating source, one file per month, regenerated from the state database, written to a temporary file and moved into place. The files therefore always reflect the current floors and filters for the games collected so far; *Rewrite collections* does this on demand.

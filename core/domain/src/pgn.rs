//! Rewrites Chess.com PGN into the Elo collection form: FIDE names, FIDE standard ratings,
//! titles and FIDE IDs replace usernames and Chess.com ratings.

/// The FIDE identity and rating written for one side.
pub struct PlayerTags<'a> {
    pub name: &'a str,
    pub elo: i32,
    pub title: &'a str,
    pub fide_id: i64,
    pub account: &'a str,
}

const DROPPED: [&str; 10] = [
    "White",
    "Black",
    "WhiteElo",
    "BlackElo",
    "CurrentPosition",
    "ECOUrl",
    "Timezone",
    "StartTime",
    "EndDate",
    "EndTime",
];
const LEADING: [&str; 4] = ["Event", "Site", "Date", "Round"];

fn parse_tag(line: &str) -> Option<(String, String)> {
    let inner = line.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (name, rest) = inner.split_once(char::is_whitespace)?;
    let rest = rest.trim().strip_prefix('"')?.strip_suffix('"')?;
    let mut value = String::with_capacity(rest.len());
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                value.push(n);
            }
        } else {
            value.push(c);
        }
    }
    Some((name.to_string(), value))
}

fn tag(out: &mut String, name: &str, value: &str) {
    out.push('[');
    out.push_str(name);
    out.push_str(" \"");
    for c in value.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        if !c.is_control() {
            out.push(c);
        }
    }
    out.push_str("\"]\n");
}

fn replace_ignore_case(text: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    let lower = text.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (i, _) in lower.match_indices(&needle) {
        if i < last {
            continue;
        }
        out.push_str(&text[last..i]);
        out.push_str(replacement);
        last = i + needle.len();
    }
    out.push_str(&text[last..]);
    out
}

/// Removes `[%clk …]` commands, dropping comments left empty.
pub fn strip_clocks(movetext: &str) -> String {
    let mut out = String::with_capacity(movetext.len());
    let mut rest = movetext;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let body = &rest[open + 1..open + close];
        let mut kept = String::new();
        let mut b = body;
        while let Some(start) = b.find("[%clk") {
            kept.push_str(&b[..start]);
            match b[start..].find(']') {
                Some(end) => b = &b[start + end + 1..],
                None => {
                    b = "";
                }
            }
        }
        kept.push_str(b);
        let kept = kept.trim();
        rest = &rest[open + close + 1..];
        if kept.is_empty() {
            rest = rest.strip_prefix(' ').unwrap_or(rest);
            if out.ends_with(' ') && rest.starts_with(|c: char| c.is_whitespace()) {
                out.pop();
            }
        } else {
            out.push('{');
            out.push_str(kept);
            out.push('}');
        }
    }
    out.push_str(rest);
    out
}

/// Tags Chess.com adds that a game database does not need.
const NOISE: [&str; 6] = ["CurrentPosition", "ECOUrl", "Timezone", "StartTime", "EndDate", "EndTime"];

/// The Chess.com-rating collection form: original players and ratings, without noise tags.
pub fn tidy(pgn: &str) -> String {
    let (tags, movetext) = split(pgn);
    let mut out = String::with_capacity(pgn.len() + 64);
    for (name, value) in &tags {
        if NOISE.contains(&name.as_str()) {
            continue;
        }
        tag(&mut out, name, value);
    }
    out.push('\n');
    out.push_str(&movetext);
    out.push('\n');
    out
}

fn split(pgn: &str) -> (Vec<(String, String)>, String) {
    let pgn = pgn.replace("\r\n", "\n");
    let mut tags: Vec<(String, String)> = Vec::new();
    let mut lines = pgn.lines().peekable();
    while let Some(line) = lines.peek() {
        if line.trim().is_empty() && tags.is_empty() {
            lines.next();
            continue;
        }
        match parse_tag(line) {
            Some(t) => {
                tags.push(t);
                lines.next();
            }
            None => break,
        }
    }
    let movetext = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (tags, movetext)
}

/// Produces the FIDE collection PGN for one game.
/// Clock comments are kept; collection export removes them when configured.
pub fn rewrite(pgn: &str, white: &PlayerTags, black: &PlayerTags) -> String {
    let (tags, movetext) = split(pgn);

    let get = |name: &str| tags.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
    let mut out = String::with_capacity(pgn.len() + 256);
    for name in LEADING {
        tag(&mut out, name, get(name).unwrap_or("?"));
    }
    tag(&mut out, "White", white.name);
    tag(&mut out, "Black", black.name);
    tag(&mut out, "Result", get("Result").unwrap_or("*"));
    tag(&mut out, "WhiteElo", &white.elo.to_string());
    tag(&mut out, "BlackElo", &black.elo.to_string());
    if !white.title.is_empty() {
        tag(&mut out, "WhiteTitle", white.title);
    }
    if !black.title.is_empty() {
        tag(&mut out, "BlackTitle", black.title);
    }
    tag(&mut out, "WhiteFideId", &white.fide_id.to_string());
    tag(&mut out, "BlackFideId", &black.fide_id.to_string());
    for (name, value) in &tags {
        if LEADING.contains(&name.as_str()) || DROPPED.contains(&name.as_str()) || name == "Result" {
            continue;
        }
        if name == "Termination" {
            let v = replace_ignore_case(value, white.account, white.name);
            tag(&mut out, name, &replace_ignore_case(&v, black.account, black.name));
        } else {
            tag(&mut out, name, value);
        }
    }
    tag(&mut out, "WhiteAccount", white.account);
    tag(&mut out, "BlackAccount", black.account);
    out.push('\n');
    out.push_str(&movetext);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[Event \"Live Chess\"]\n[Site \"Chess.com\"]\n[Date \"2026.09.08\"]\n[Round \"-\"]\n[White \"michobr\"]\n[Black \"Hikaru\"]\n[Result \"0-1\"]\n[CurrentPosition \"8/8 w - - 2 37\"]\n[Timezone \"UTC\"]\n[ECO \"B22\"]\n[ECOUrl \"https://www.chess.com/openings/x\"]\n[UTCDate \"2026.09.08\"]\n[WhiteElo \"2709\"]\n[BlackElo \"3370\"]\n[TimeControl \"300\"]\n[Termination \"Hikaru won by checkmate\"]\n[Link \"https://www.chess.com/game/live/1\"]\n\n1. e4 {[%clk 0:04:53.5]} 1... c5 {[%clk 0:04:54.8]} 2. Qh5 {note [%clk 0:04:52]} 0-1\n";

    fn players() -> (PlayerTags<'static>, PlayerTags<'static>) {
        (
            PlayerTags { name: "Bro, Michal", elo: 2512, title: "IM", fide_id: 1, account: "michobr" },
            PlayerTags { name: "Nakamura, Hikaru", elo: 2792, title: "GM", fide_id: 2016192, account: "Hikaru" },
        )
    }

    #[test]
    fn rewrites_identity_and_ratings() {
        let (w, b) = players();
        let out = rewrite(SAMPLE, &w, &b);
        assert!(out.starts_with("[Event \"Live Chess\"]\n[Site \"Chess.com\"]\n[Date \"2026.09.08\"]\n[Round \"-\"]\n[White \"Bro, Michal\"]\n[Black \"Nakamura, Hikaru\"]\n[Result \"0-1\"]\n[WhiteElo \"2512\"]\n[BlackElo \"2792\"]\n"));
        assert!(out.contains("[BlackFideId \"2016192\"]"));
        assert!(out.contains("[Termination \"Nakamura, Hikaru won by checkmate\"]"));
        assert!(out.contains("[WhiteAccount \"michobr\"]"));
        assert!(!out.contains("3370") && !out.contains("CurrentPosition") && !out.contains("ECOUrl"));
        assert!(out.contains("{[%clk 0:04:53.5]}"));
        assert!(out.ends_with("0-1\n"));
    }

    #[test]
    fn tidies_chesscom_form() {
        let out = tidy(SAMPLE);
        assert!(
            out.contains("[White \"michobr\"]\n[Black \"Hikaru\"]")
                && out.contains("[BlackElo \"3370\"]\n[TimeControl")
        );
        assert!(!out.contains("CurrentPosition") && !out.contains("Timezone") && !out.contains("FideId"));
    }

    #[test]
    fn strips_clock_comments() {
        assert_eq!(
            strip_clocks("1. e4 {[%clk 0:04:53.5]} 1... c5 {[%clk 0:04:54.8]} 2. Qh5 {note [%clk 0:04:52]} 0-1"),
            "1. e4 1... c5 2. Qh5 {note} 0-1"
        );
    }

    #[test]
    fn escapes_values() {
        let mut out = String::new();
        tag(&mut out, "Event", "A \"quoted\" \\ event");
        assert_eq!(out, "[Event \"A \\\"quoted\\\" \\\\ event\"]\n");
        assert_eq!(parse_tag(out.trim()).unwrap().1, "A \"quoted\" \\ event");
    }
}

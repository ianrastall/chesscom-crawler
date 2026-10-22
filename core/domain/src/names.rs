//! Name normalization and matching between Chess.com profile names and FIDE list names.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Lower-case ASCII-folded word tokens of a personal name.
pub fn tokens(name: &str) -> Vec<String> {
    let mut folded = String::with_capacity(name.len());
    for c in name.nfkd() {
        if is_combining_mark(c) {
            continue;
        }
        match c {
            'ø' | 'Ø' => folded.push('o'),
            'đ' | 'Đ' | 'ð' | 'Ð' => folded.push('d'),
            'ł' | 'Ł' => folded.push('l'),
            'ß' => folded.push_str("ss"),
            'æ' | 'Æ' => folded.push_str("ae"),
            'œ' | 'Œ' => folded.push_str("oe"),
            'þ' | 'Þ' => folded.push_str("th"),
            'ı' => folded.push('i'),
            c if c.is_alphanumeric() => folded.extend(c.to_lowercase()),
            // Apostrophes join a name ("O'Brien" -> "obrien"); other punctuation separates words.
            '\'' | '’' | '`' => {}
            _ => folded.push(' '),
        }
    }
    // German transliterations ("Koelle" for "Kölle") fold to the same key on both sides.
    folded.split_whitespace().map(|t| t.replace("ae", "a").replace("oe", "o").replace("ue", "u")).collect()
}

const HONORIFICS: [&str; 11] = ["gm", "im", "fm", "cm", "wgm", "wim", "wfm", "wcm", "nm", "dr", "prof"];

/// Tokens of a Chess.com profile name, without a leading title or honorific ("GM Magnus Carlsen").
pub fn profile_tokens(name: &str) -> Vec<String> {
    let mut t = tokens(name);
    while t.len() > 2 && HONORIFICS.contains(&t[0].as_str()) {
        t.remove(0);
    }
    t
}

/// A FIDE list name ("Carlsen, Magnus") split into surname and given-name tokens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FideName {
    pub surname: Vec<String>,
    pub given: Vec<String>,
}

impl FideName {
    pub fn parse(name: &str) -> Self {
        match name.split_once(',') {
            Some((surname, given)) => FideName { surname: tokens(surname), given: tokens(given) },
            None => FideName { surname: tokens(name), given: Vec::new() },
        }
    }

    pub fn all_tokens(&self) -> impl Iterator<Item = &String> {
        self.surname.iter().chain(self.given.iter())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchKind {
    /// The first surname and first given name are present, other words agree allowing for
    /// initials, truncation and small spelling differences, and at most one profile word
    /// (of three or more) is unaccounted for. Used only when nothing closer exists.
    Loose,
    /// The profile name contains every surname token and the first given name, and nothing
    /// that is absent from the FIDE name (middle names may be omitted).
    Partial,
    /// The same words in any order.
    Exact,
}

/// How a profile name relates to a FIDE name, if at all.
pub fn match_kind(profile: &[String], fide: &FideName) -> Option<MatchKind> {
    if profile.len() < 2 || fide.surname.is_empty() {
        return None;
    }
    let mut a: Vec<&str> = profile.iter().map(String::as_str).collect();
    let mut b: Vec<&str> = fide.all_tokens().map(String::as_str).collect();
    a.sort_unstable();
    b.sort_unstable();
    if a == b {
        return Some(MatchKind::Exact);
    }
    let has = |t: &String| profile.contains(t);
    let surname_present = fide.surname.iter().all(has);
    let given_present = fide.given.first().is_none_or(has);
    let nothing_extra = profile.iter().all(|t| fide.all_tokens().any(|f| f == t));
    if surname_present && given_present && nothing_extra {
        return Some(MatchKind::Partial);
    }
    loose(profile, fide).then_some(MatchKind::Loose)
}

fn loose(profile: &[String], fide: &FideName) -> bool {
    let Some(surname) = fide.surname.first() else { return false };
    let Some(at) = profile.iter().position(|t| t == surname) else { return false };
    if let Some(given) = fide.given.first()
        && !profile.iter().enumerate().any(|(i, t)| i != at && compatible(t, given))
    {
        return false;
    }
    let unmatched = profile.iter().filter(|p| !fide.all_tokens().any(|f| compatible(p, f))).count();
    unmatched <= usize::from(profile.len() >= 3)
}

/// Equal words, an initial, a truncation of at least four letters, or a near spelling.
fn compatible(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if long.starts_with(short) && (short.len() == 1 || short.len() >= 4) {
        return true;
    }
    if short.len() < 5 || a.chars().next() != b.chars().next() {
        return false;
    }
    let limit = if long.len() <= 7 { 1 } else { 2 };
    levenshtein(a, b) <= limit
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Vec<String> {
        profile_tokens(s)
    }

    #[test]
    fn folds_accents_and_punctuation() {
        assert_eq!(tokens("Maxime Vachier-Lagrave"), vec!["maxime", "vachier", "lagrave"]);
        assert_eq!(tokens("Jan-Krzysztof Duda"), vec!["jan", "krzysztof", "duda"]);
        assert_eq!(tokens("Łukasz Józef Ørsted"), vec!["lukasz", "jozef", "orsted"]);
        assert_eq!(tokens("Tobias Kölle"), vec!["tobias", "kolle"]);
        assert_eq!(tokens("Koelle, Tobias"), vec!["kolle", "tobias"]);
        assert_eq!(tokens("O'Brien, Seán"), vec!["obrien", "sean"]);
        assert_eq!(t("GM Magnus Carlsen"), vec!["magnus", "carlsen"]);
        assert_eq!(t("Im Jae"), vec!["im", "ja"]);
    }

    #[test]
    fn matches_names() {
        let carlsen = FideName::parse("Carlsen, Magnus");
        assert_eq!(match_kind(&t("Magnus Carlsen"), &carlsen), Some(MatchKind::Exact));
        assert_eq!(match_kind(&t("Carlsen Magnus"), &carlsen), Some(MatchKind::Exact));
        assert_eq!(match_kind(&t("Magnus"), &carlsen), None);
        assert_eq!(match_kind(&t("Magnus Carlsen Fan"), &carlsen), Some(MatchKind::Loose));
        assert_eq!(match_kind(&t("Magnus Carlsen Fan Club"), &carlsen), None);

        let mvl = FideName::parse("Vachier-Lagrave, Maxime");
        assert_eq!(match_kind(&t("Maxime Vachier-Lagrave"), &mvl), Some(MatchKind::Exact));

        let loose = |p: &str, f: &str| match_kind(&t(p), &FideName::parse(f));
        assert_eq!(loose("Oleksandr Bortnyk", "Bortnyk, Olexandr"), Some(MatchKind::Loose));
        assert_eq!(
            loose("Christian Amilcar Santiago Vilca", "Santiago Vilca, Christian Amilca"),
            Some(MatchKind::Loose)
        );
        assert_eq!(loose("Logan Clark Shafer", "Shafer, Logan C"), Some(MatchKind::Loose));
        assert_eq!(loose("Gabriel Rueda", "Rueda Ahumada, Gabriel Omar"), Some(MatchKind::Loose));
        assert_eq!(loose("Gianmarco Leiva Rodríguez", "Leiva, Gianmarco"), Some(MatchKind::Loose));
        assert_eq!(loose("Tobias Kölle", "Koelle, Tobias"), Some(MatchKind::Exact));
        assert_eq!(loose("Mykola Bortnyk", "Bortnyk, Olexandr"), None);
        assert_eq!(loose("Magnus Carlsen Fan Club", "Carlsen, Magnus"), None);
        assert_eq!(loose("Gabriel Ruedas", "Rueda Ahumada, Gabriel Omar"), None);

        let middle = FideName::parse("Smith, John Patrick");
        assert_eq!(match_kind(&t("John Smith"), &middle), Some(MatchKind::Partial));
        assert_eq!(match_kind(&t("Patrick Smith"), &middle), None);
    }
}

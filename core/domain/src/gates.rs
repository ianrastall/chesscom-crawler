//! Inclusion gates. Local gates use only the game record; identity and rating gates need FIDE
//! resolution. Every gate must pass, so evaluation order changes only how many requests are made.

use crate::chesscom::Game;
use crate::settings::Settings;

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    Variant,
    TimeClass,
    Unrated,
    NotCheckmate,
    Untitled,
    Prefilter,
    Unresolved,
    FairPlay,
    SamePlayer,
    NoFideList,
    BelowFloor,
}

impl Rejection {
    pub fn code(self) -> &'static str {
        match self {
            Rejection::Variant => "variant",
            Rejection::TimeClass => "time_class",
            Rejection::Unrated => "unrated",
            Rejection::NotCheckmate => "not_checkmate",
            Rejection::Untitled => "untitled",
            Rejection::Prefilter => "prefilter",
            Rejection::Unresolved => "unresolved",
            Rejection::FairPlay => "fair_play",
            Rejection::SamePlayer => "same_player",
            Rejection::NoFideList => "no_fide_list",
            Rejection::BelowFloor => "below_floor",
        }
    }
}

/// Game properties that later output filtering can re-test without the original record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameFacts<'a> {
    pub time_class: &'a str,
    pub rated: bool,
    pub standard: bool,
    pub checkmate: bool,
}

impl<'a> GameFacts<'a> {
    pub fn of(game: &'a Game) -> Self {
        GameFacts {
            time_class: &game.time_class,
            rated: game.rated,
            standard: game.rules == "chess" && (game.initial_setup.is_empty() || game.initial_setup == START_FEN),
            checkmate: game.white.result == "checkmated" || game.black.result == "checkmated",
        }
    }

    pub fn check(&self, settings: &Settings) -> Result<(), Rejection> {
        if settings.standard_only && !self.standard {
            return Err(Rejection::Variant);
        }
        if !settings.time_classes.allows(self.time_class) {
            return Err(Rejection::TimeClass);
        }
        if settings.rated_only && !self.rated {
            return Err(Rejection::Unrated);
        }
        if settings.checkmate_only && !self.checkmate {
            return Err(Rejection::NotCheckmate);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chesscom::Side;

    fn game() -> Game {
        Game {
            rules: "chess".into(),
            time_class: "blitz".into(),
            rated: true,
            initial_setup: START_FEN.into(),
            white: Side { result: "win".into(), ..Side::default() },
            black: Side { result: "checkmated".into(), ..Side::default() },
            ..Game::default()
        }
    }

    #[test]
    fn local_gates() {
        let mut s = Settings::default();
        let g = game();
        assert_eq!(GameFacts::of(&g).check(&s), Ok(()));
        let mut v = game();
        v.rules = "chess960".into();
        assert_eq!(GameFacts::of(&v).check(&s), Err(Rejection::Variant));
        let mut b = game();
        b.time_class = "bullet".into();
        assert_eq!(GameFacts::of(&b).check(&s), Err(Rejection::TimeClass));
        let mut r = game();
        r.black.result = "resigned".into();
        assert_eq!(GameFacts::of(&r).check(&s), Ok(()));
        s.checkmate_only = true;
        assert_eq!(GameFacts::of(&r).check(&s), Err(Rejection::NotCheckmate));
        let mut u = game();
        u.rated = false;
        assert_eq!(GameFacts::of(&u).check(&s), Err(Rejection::Unrated));
        let mut odd = game();
        odd.initial_setup = "8/8/8/8/8/8/8/K6k w - - 0 1".into();
        assert_eq!(GameFacts::of(&odd).check(&s), Err(Rejection::Variant));
    }
}

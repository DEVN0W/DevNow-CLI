use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    pub fn from_arg(arg: Option<&str>) -> Result<Self, String> {
        match arg.unwrap_or("normal").to_ascii_lowercase().as_str() {
            "easy" => Ok(Self::Easy),
            "normal" => Ok(Self::Normal),
            "hard" => Ok(Self::Hard),
            other => Err(format!(
                "unknown difficulty '{other}'. Use easy, normal, or hard."
            )),
        }
    }

    pub fn attempt_limit(self) -> u8 {
        match self {
            Self::Easy => 10,
            Self::Normal => 7,
            Self::Hard => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Easy => "easy",
            Self::Normal => "normal",
            Self::Hard => "hard",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuessOutcome {
    Correct,
    TooLow { delta: u8, warmer: Option<bool> },
    TooHigh { delta: u8, warmer: Option<bool> },
    OutOfRange,
    AlreadyGuessed,
    GameOver,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuessReport {
    pub outcome: GuessOutcome,
    pub attempts_used: u8,
    pub attempts_left: u8,
}

#[derive(Debug, Clone)]
pub struct Game {
    secret: u8,
    max_attempts: u8,
    guesses: Vec<u8>,
    won: bool,
}

impl Game {
    pub fn new(secret: u8, difficulty: Difficulty) -> Self {
        Self {
            secret,
            max_attempts: difficulty.attempt_limit(),
            guesses: Vec::new(),
            won: false,
        }
    }

    pub fn attempts_left(&self) -> u8 {
        self.max_attempts.saturating_sub(self.guesses.len() as u8)
    }

    pub fn guesses(&self) -> &[u8] {
        &self.guesses
    }

    pub fn is_over(&self) -> bool {
        self.won || self.attempts_left() == 0
    }

    pub fn reveal_secret(&self) -> u8 {
        self.secret
    }

    pub fn hint(&self) -> &'static str {
        match self.secret.cmp(&50) {
            Ordering::Less => "The target is below 50.",
            Ordering::Equal => "The target is exactly 50.",
            Ordering::Greater => "The target is above 50.",
        }
    }

    pub fn submit_guess(&mut self, guess: u8) -> GuessReport {
        if !(1..=100).contains(&guess) {
            return self.report(GuessOutcome::OutOfRange);
        }
        if self.won || self.attempts_left() == 0 {
            return self.report(GuessOutcome::GameOver);
        }
        if self.guesses.contains(&guess) {
            return self.report(GuessOutcome::AlreadyGuessed);
        }

        let previous_delta = self.guesses.last().map(|last| abs_diff(*last, self.secret));
        let delta = abs_diff(guess, self.secret);
        self.guesses.push(guess);

        let outcome = if delta == 0 {
            self.won = true;
            GuessOutcome::Correct
        } else {
            let warmer = previous_delta.map(|previous| delta < previous);
            if guess < self.secret {
                GuessOutcome::TooLow { delta, warmer }
            } else {
                GuessOutcome::TooHigh { delta, warmer }
            }
        };

        self.report(outcome)
    }

    fn report(&self, outcome: GuessOutcome) -> GuessReport {
        GuessReport {
            outcome,
            attempts_used: self.guesses.len() as u8,
            attempts_left: self.attempts_left(),
        }
    }
}

fn abs_diff(lhs: u8, rhs: u8) -> u8 {
    lhs.abs_diff(rhs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn difficulty_defaults_to_normal() {
        assert_eq!(Difficulty::from_arg(None), Ok(Difficulty::Normal));
    }

    #[test]
    fn first_guess_has_no_temperature_comparison() {
        let mut game = Game::new(40, Difficulty::Normal);
        let report = game.submit_guess(10);
        assert_eq!(
            report.outcome,
            GuessOutcome::TooLow {
                delta: 30,
                warmer: None
            }
        );
    }

    #[test]
    fn second_guess_reports_warmer_state() {
        let mut game = Game::new(40, Difficulty::Normal);
        game.submit_guess(10);
        let report = game.submit_guess(25);
        assert_eq!(
            report.outcome,
            GuessOutcome::TooLow {
                delta: 15,
                warmer: Some(true)
            }
        );
    }

    #[test]
    fn repeated_guess_does_not_spend_attempts() {
        let mut game = Game::new(40, Difficulty::Normal);
        game.submit_guess(10);
        let report = game.submit_guess(10);
        assert_eq!(report.outcome, GuessOutcome::AlreadyGuessed);
        assert_eq!(report.attempts_used, 1);
        assert_eq!(report.attempts_left, 6);
    }

    #[test]
    fn out_of_range_guess_does_not_spend_attempts() {
        let mut game = Game::new(40, Difficulty::Normal);
        let report = game.submit_guess(0);
        assert_eq!(report.outcome, GuessOutcome::OutOfRange);
        assert_eq!(report.attempts_used, 0);
        assert_eq!(report.attempts_left, 7);
    }

    #[test]
    fn hard_mode_ends_after_five_misses() {
        let mut game = Game::new(99, Difficulty::Hard);
        for guess in [1, 2, 3, 4, 5] {
            game.submit_guess(guess);
        }
        assert!(game.is_over());
        assert_eq!(game.attempts_left(), 0);
        assert_eq!(game.reveal_secret(), 99);
    }
}

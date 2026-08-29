use std::env;
use std::io;
use std::io::Write;

use number_nexus::Difficulty;
use number_nexus::Game;
use number_nexus::GuessOutcome;
use rand::Rng;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let difficulty = match Difficulty::from_arg(env::args().nth(1).as_deref()) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            std::process::exit(2);
        }
    };

    let secret = rand::rng().random_range(1..=100);
    let mut game = Game::new(secret, difficulty);

    println!("Number Nexus");
    println!(
        "Difficulty: {}. Guess the hidden number from 1 to 100 in {} attempts.",
        difficulty.label(),
        difficulty.attempt_limit()
    );
    println!("Commands: type a number, 'hint', 'history', or 'quit'.");

    let stdin = io::stdin();
    loop {
        if game.is_over() {
            if game.attempts_left() == 0 {
                println!(
                    "You are out of attempts. The number was {}.",
                    game.reveal_secret()
                );
            }
            break;
        }

        print!("guess> ");
        io::stdout().flush()?;

        let mut input = String::new();
        if stdin.read_line(&mut input)? == 0 {
            println!();
            break;
        }
        let trimmed = input.trim();

        if trimmed.eq_ignore_ascii_case("quit") {
            println!("You gave up. The number was {}.", game.reveal_secret());
            break;
        }
        if trimmed.eq_ignore_ascii_case("hint") {
            println!("{}", game.hint());
            continue;
        }
        if trimmed.eq_ignore_ascii_case("history") {
            if game.guesses().is_empty() {
                println!("No guesses yet.");
            } else {
                println!("Previous guesses: {:?}", game.guesses());
            }
            continue;
        }

        let guess = match trimmed.parse::<u8>() {
            Ok(value) => value,
            Err(_) => {
                println!("Enter a whole number from 1 to 100, or use hint/history/quit.");
                continue;
            }
        };

        let report = game.submit_guess(guess);
        match report.outcome {
            GuessOutcome::Correct => {
                println!(
                    "Direct hit. You solved it in {} attempts.",
                    report.attempts_used
                );
                break;
            }
            GuessOutcome::TooLow { delta, warmer } => {
                println!("{}", describe_distance("low", delta, warmer));
            }
            GuessOutcome::TooHigh { delta, warmer } => {
                println!("{}", describe_distance("high", delta, warmer));
            }
            GuessOutcome::OutOfRange => {
                println!("That guess is outside the 1 to 100 range.");
                continue;
            }
            GuessOutcome::AlreadyGuessed => {
                println!("You already tried that number.");
                continue;
            }
            GuessOutcome::GameOver => {
                println!("The game is already over.");
                break;
            }
        }

        println!("Attempts left: {}", report.attempts_left);
    }

    Ok(())
}

fn print_usage() {
    eprintln!("Usage: cargo run -p number-nexus -- [easy|normal|hard]");
}

fn describe_distance(direction: &str, delta: u8, warmer: Option<bool>) -> String {
    let distance = match delta {
        1..=3 => "Burning hot",
        4..=10 => "Hot",
        11..=20 => "Warm",
        _ => "Cold",
    };

    let trend = match warmer {
        Some(true) => " and warmer than your last guess",
        Some(false) => " and colder than your last guess",
        None => "",
    };

    format!("{distance}: too {direction}{trend}.")
}

#[cfg(test)]
mod tests {
    use super::describe_distance;

    #[test]
    fn describe_distance_includes_temperature_and_trend() {
        assert_eq!(
            describe_distance("high", 2, Some(false)),
            "Burning hot: too high and colder than your last guess."
        );
    }
}

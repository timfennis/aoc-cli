use anyhow::{anyhow, Context};
use chrono::prelude::*;
use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::borrow::Cow;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::Command;

#[derive(Deserialize, Debug)]
struct Config {
    cookie: String,
    year: Option<i32>,
}

#[derive(Parser)]
struct Cli {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
#[command(author = "Tim Fennis <fennis.tim@gmail.com>")]
enum Commands {
    /// Initialize a new script for this day
    Init { day: Option<u32> },
    /// Run tests for this day (not implemented)
    Test { day: Option<u32> },
    /// Run this day's solution
    Run { day: Option<u32> },
    /// Get the input for the given day's puzzle
    Get { day: Option<u32> },
    /// Preview the puzzle input for the given day
    Preview { day: Option<u32> },
    /// Print out the entire input for the given day
    Print { day: Option<u32> },
}

impl Commands {
    pub fn day(&self) -> Option<u32> {
        match self {
            Commands::Init { day } => *day,
            Commands::Test { day } => *day,
            Commands::Run { day } => *day,
            Commands::Get { day } => *day,
            Commands::Preview { day } => *day,
            Commands::Print { day } => *day,
        }
    }
}

#[derive(Debug, Clone)]
struct Puzzle {
    year: i32,
    day: u32,
}

impl Puzzle {
    pub fn input_path(&self) -> String {
        format!("input/{}/{}.txt", self.year, self.day)
    }
}

#[derive(Debug, Clone)]
enum Action {
    Init(Puzzle),
    Test(Puzzle),
    Run(Puzzle),
    Get(Puzzle),
    Preview(Puzzle),
    Print(Puzzle),
}

impl Action {
    fn execute(&self, config: &Config) -> anyhow::Result<()> {
        match self {
            Action::Init(puzzle) => init_puzzle(puzzle),
            Action::Run(puzzle) => run_puzzle(puzzle),
            Action::Get(puzzle) => get_puzzle(puzzle, &config.cookie).map(|_| ()),
            Action::Preview(puzzle) => preview_puzzle(puzzle),
            Action::Print(puzzle) => print_puzzle(puzzle),
            Action::Test(puzzle) => test_puzzle(puzzle),
        }
    }
}

fn test_puzzle(_puzzle: &Puzzle) -> anyhow::Result<()> {
    todo!("not yet implemented")
}

impl TryFrom<(i32, Option<u32>, Commands)> for Action {
    type Error = anyhow::Error;

    fn try_from(
        (year, auto_day, command): (i32, Option<u32>, Commands),
    ) -> Result<Self, Self::Error> {
        let day = command
            .day()
            .or(auto_day)
            .context("cannot infer day outside of december")?;
        let action = match command {
            Commands::Init { .. } => Action::Init(Puzzle { year, day }),
            Commands::Test { .. } => Action::Test(Puzzle { year, day }),
            Commands::Run { .. } => Action::Run(Puzzle { year, day }),
            Commands::Get { .. } => Action::Get(Puzzle { year, day }),
            Commands::Preview { .. } => Action::Preview(Puzzle { year, day }),
            Commands::Print { .. } => Action::Print(Puzzle { year, day }),
        };

        Ok(action)
    }
}

fn main() -> anyhow::Result<()> {
    // try to load env file, but we don't care if it fails
    let _ = dotenvy::dotenv();

    let config = envy::from_env::<Config>()?;

    // Get the current local date and time
    let local_date = Local::now();

    // Extract the year, month, and day
    let mut year = config.year;
    let month = local_date.month(); // 1-12
    let mut day = None;

    // If 'tis the season we infer day and year if possible
    if month == 12 && (1u32..=25).contains(&local_date.day()) {
        year = year.or(Some(local_date.year()));
        day = Some(local_date.day())
    }

    let year = year.context("cannot infer year outside of december, please set YEAR env var")?;

    let cli = Cli::parse();
    let action = match cli.command {
        Some(cmd) => Action::try_from((year, day, cmd)),
        None => Action::try_from((year, day, Commands::Run { day })),
    }?;

    action.execute(&config)?;

    Ok(())
}

fn init_puzzle(puzzle: &Puzzle) -> anyhow::Result<()> {
    let (year, day) = (puzzle.year, puzzle.day);

    let solution_path = format!("{day}.ndc");
    let solution_file = Path::new(&solution_path);

    if !solution_file.exists() {
        let mut file =
            std::fs::File::create(solution_file).context("cannot create solution file")?;
        write!(file, "let input = read_file(\"input/{year}/{day}.txt\");\n\nfor line in input.lines() {{\n\tprint(line);\n}}").context("cannot write to file")?;
    }

    Ok(())
}
fn run_puzzle(puzzle: &Puzzle) -> anyhow::Result<()> {
    let day = puzzle.day;
    let solution_path = format!("{day}.ndc");
    let solution_file = Path::new(&solution_path);

    if !solution_file.exists() {
        return Err(anyhow!(
            "{solution_file:?} not found, run init subcommand to create it"
        ));
    }

    let cmd = Command::new("ndc")
        .arg(solution_file)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn child")?;

    let stdout = cmd.stdout.context("cannot get stdout")?;
    let mut stderr = cmd.stderr.context("cannot get stderr")?;

    let mut out_buff = String::new();
    let mut reader = BufReader::new(stdout);

    loop {
        let mut line_buffer = String::new();
        match reader.read_line(&mut line_buffer) {
            Ok(0) => break,
            Err(err) => return Err(anyhow!("{err}")),
            Ok(_) => {
                print!("{line_buffer}");
                std::io::stdout()
                    .flush()
                    .context("failed to flush stdout")?;
                out_buff.push_str(&line_buffer);
            }
        }
    }

    let mut err_buff = String::new();
    stderr
        .read_to_string(&mut err_buff)
        .context("failed to read stderr")?;

    eprint!("{err_buff}");
    std::io::stderr()
        .flush()
        .context("failed to flush stderr")?;

    Ok(())
}

fn print_puzzle(puzzle: &Puzzle) -> anyhow::Result<()> {
    let input_file = puzzle.input_path();

    if !std::fs::exists(&input_file).context("failed to check if file exists")? {
        return Err(anyhow!("input file not downloaded"));
    }

    let file = std::fs::read_to_string(&input_file).context("failed to read input file")?;

    println!("{file}");

    Ok(())
}

fn preview_puzzle(puzzle: &Puzzle) -> anyhow::Result<()> {
    let input_file = puzzle.input_path();

    if !std::fs::exists(&input_file).context("failed to check if file exists")? {
        return Err(anyhow!("input file not downloaded"));
    }

    let file = std::fs::read_to_string(&input_file).context("failed to read input file")?;

    let line_count = file.lines().count();

    let len = usize::try_from((line_count + 1).ilog10() + 2)
        .context("failed to calc line number width")?;

    let truncate = line_count > 50;

    let max_w = term_size::dimensions().map(|(w, _)| w).unwrap_or(10_000);
    let mut line_length_list = Vec::new();

    for (nr, line) in file.lines().enumerate() {
        line_length_list.push(line.len() as i32);
        let line = if line.len() > max_w {
            Cow::Owned(
                line.chars()
                    .take(max_w - len - 5)
                    .chain("..".chars())
                    .collect::<String>(),
            )
        } else {
            Cow::Borrowed(line)
        };

        let nr = nr + 1;
        if nr <= 20 || nr >= (line_count - 20) || !truncate {
            println!("{:>width$} | {line}", nr, width = len)
        } else if nr == 22 {
            let truncated = line_count - 40;
            println!(
                "{:>width$} | <<{truncated} lines truncated>>",
                "..",
                width = len
            );
        } else if nr == 21 || nr == 23 {
            println!("{:>width$} | ... ", "..", width = len);
        }
    }

    line_length_list.sort();

    if line_length_list.len() > 0 {
        let median = if line_length_list.len() % 2 == 0 {
            let idx = line_length_list.len().div_euclid(2);
            f64::from(line_length_list[idx] + line_length_list[idx + 1]) / 2.0
        } else {
            let idx = line_length_list.len().div_ceil(2);
            f64::from(line_length_list[idx])
        };

        println!("\nMax line length: {}", line_length_list.last().unwrap());
        println!("Median line length: {median}");
        println!("Min line length: {}", line_length_list.first().unwrap());
    } else {
        println!("\nInput is empty");
    }
    Ok(())
}

fn get_puzzle(puzzle: &Puzzle, cookie_value: &str) -> anyhow::Result<()> {
    let (year, day) = (puzzle.year, puzzle.day);

    // TODO: write some logic to check if we can even download the puzzle (see if start time has passed)

    let output_file = puzzle.input_path();

    if std::fs::exists(&output_file).context("failed to check if file exists")? {
        std::fs::create_dir_all(format!("input/{year}/"))
            .context("cannot create puzzle directory")?;

        let response = ureq::get(&format!("https://adventofcode.com/{year}/day/{day}/input"))
            .set("Cookie", cookie_value)
            .set("User-Agent", "aoc-cli/0.0.1 fennis.tim@gmail.com")
            .call()
            .context("cannot create request")?;

        let status = response.status();

        if (200u16..300u16).contains(&status) {
            let text = response
                .into_string()
                .context("cannot parse response as text")?;

            std::fs::write(&output_file, text).context("cannot write puzzle output")?;
        } else {
            return Err(anyhow!("ERR: didn't receive 200 status from server"));
        }
    }

    preview_puzzle(puzzle)
}

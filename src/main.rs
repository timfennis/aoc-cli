use anyhow::{Context, anyhow, bail};
use chrono::{DateTime, Datelike, FixedOffset, Local, TimeZone, Utc};
use clap::{Parser, Subcommand};
use std::borrow::Cow;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const USER_AGENT: &str = concat!(
    "aoc-cli/",
    env!("CARGO_PKG_VERSION"),
    " fennis.tim@gmail.com"
);

/// Template written by `aoc init`
const SOLUTION_TEMPLATE: &str =
    "let input = read_stdin();\n\nfor line in input.lines() {\n\tprint(line);\n}\n";

/// Advent of Code puzzles unlock at midnight US Eastern time (UTC-5, there is no DST in December)
fn aoc_timezone() -> FixedOffset {
    FixedOffset::west_opt(5 * 3600).expect("UTC-5 is a valid offset")
}

#[derive(Parser)]
#[command(about = "Advent of Code helper for ndc solutions")]
struct Cli {
    /// Puzzle year. Defaults to the year directory you're in, then $YEAR, then the current December
    #[arg(short, long, global = true)]
    year: Option<i32>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
#[command(author = "Tim Fennis <fennis.tim@gmail.com>")]
enum Commands {
    /// Initialize a new script for this day
    Init { day: Option<u32> },
    /// Run tests for this day (not implemented)
    Test { day: Option<u32> },
    /// Run this day's solution with the puzzle input on stdin
    Run { day: Option<u32> },
    /// Get the input for the given day's puzzle
    Get { day: Option<u32> },
    /// Preview the puzzle input for the given day
    Preview { day: Option<u32> },
    /// Print out the entire input for the given day
    Print { day: Option<u32> },
}

impl Commands {
    fn day(&self) -> Option<u32> {
        match self {
            Commands::Init { day }
            | Commands::Test { day }
            | Commands::Run { day }
            | Commands::Get { day }
            | Commands::Preview { day }
            | Commands::Print { day } => *day,
        }
    }
}

/// The monorepo holding solutions in `<root>/<year>/<day>.ndc` and inputs in `<root>/input/<year>/<day>.txt`
struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// The root is `$AOC_ROOT` if set, otherwise the directory holding the `.env` file, otherwise the working directory
    fn discover(env_file: Option<PathBuf>) -> anyhow::Result<Self> {
        let root = match (std::env::var_os("AOC_ROOT"), env_file) {
            (Some(root), _) => PathBuf::from(root),
            (None, Some(env_file)) => env_file
                .parent()
                .context(".env file has no parent directory")?
                .to_path_buf(),
            (None, None) => {
                std::env::current_dir().context("cannot determine working directory")?
            }
        };
        let root = root
            .canonicalize()
            .with_context(|| format!("AoC root {} does not exist", root.display()))?;

        Ok(Self { root })
    }

    /// The year of the `<root>/<year>/` directory we're currently in, if any
    fn year_from_cwd(&self) -> Option<i32> {
        let cwd = std::env::current_dir().ok()?.canonicalize().ok()?;
        let first = cwd.strip_prefix(&self.root).ok()?.components().next()?;
        first
            .as_os_str()
            .to_str()?
            .parse()
            .ok()
            .filter(|year| *year >= FIRST_YEAR)
    }

    fn solution_path(&self, puzzle: &Puzzle) -> PathBuf {
        self.root
            .join(puzzle.year.to_string())
            .join(format!("{}.ndc", puzzle.day))
    }

    fn input_path(&self, puzzle: &Puzzle) -> PathBuf {
        self.root
            .join("input")
            .join(puzzle.year.to_string())
            .join(format!("{}.txt", puzzle.day))
    }

    fn read_input(&self, puzzle: &Puzzle) -> anyhow::Result<String> {
        let path = self.input_path(puzzle);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(text),
            Err(err) if err.kind() == ErrorKind::NotFound => bail!(
                "input not downloaded ({}), run `aoc get {} -y {}`",
                path.display(),
                puzzle.day,
                puzzle.year
            ),
            Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
        }
    }
}

const FIRST_YEAR: i32 = 2015;

#[derive(Debug, Clone)]
struct Puzzle {
    year: i32,
    day: u32,
}

impl Puzzle {
    fn new(year: i32, day: u32) -> anyhow::Result<Self> {
        if year < FIRST_YEAR {
            bail!("Advent of Code started in {FIRST_YEAR}, got year {year}");
        }
        let days = Self::days_in_year(year);
        if !(1..=days).contains(&day) {
            bail!("{year} has puzzles for days 1-{days}, got day {day}");
        }
        Ok(Self { year, day })
    }

    /// Advent of Code has 25 puzzles per year through 2024 and 12 from 2025 onwards
    fn days_in_year(year: i32) -> u32 {
        if year >= 2025 { 12 } else { 25 }
    }

    fn unlocks_at(&self) -> DateTime<FixedOffset> {
        aoc_timezone()
            .with_ymd_and_hms(self.year, 12, self.day, 0, 0, 0)
            .single()
            .expect("validated puzzle dates exist")
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let env_file = match dotenvy::dotenv() {
        Ok(path) => Some(path),
        Err(err) if err.not_found() => None,
        Err(err) => return Err(err).context("failed to load .env file"),
    };
    let workspace = Workspace::discover(env_file)?;

    let now = Utc::now().with_timezone(&aoc_timezone());
    let in_season = now.month() == 12;

    let year = match cli.year.or_else(|| workspace.year_from_cwd()) {
        Some(year) => Some(year),
        None => env_year()?,
    }
    .or(in_season.then(|| now.year()))
    .context(
        "cannot infer year outside of December: pass --year, run from a year directory or set YEAR",
    )?;

    let command = cli.command.unwrap_or(Commands::Run { day: None });

    // Only infer the day when working on this December's event
    let today = (in_season && year == now.year() && now.day() <= Puzzle::days_in_year(year))
        .then(|| now.day());
    let day = command
        .day()
        .or(today)
        .context("cannot infer day outside of the current event, pass it explicitly")?;

    let puzzle = Puzzle::new(year, day)?;

    match command {
        Commands::Init { .. } => init_puzzle(&workspace, &puzzle),
        Commands::Test { .. } => bail!("`test` is not implemented yet"),
        Commands::Run { .. } => run_puzzle(&workspace, &puzzle),
        Commands::Get { .. } => {
            get_puzzle(&workspace, &puzzle, &session_cookie()?)?;
            preview_puzzle(&workspace, &puzzle)
        }
        Commands::Preview { .. } => preview_puzzle(&workspace, &puzzle),
        Commands::Print { .. } => print_puzzle(&workspace, &puzzle),
    }
}

fn env_year() -> anyhow::Result<Option<i32>> {
    match std::env::var("YEAR") {
        Ok(year) => year
            .trim()
            .parse()
            .map(Some)
            .with_context(|| format!("YEAR must be a number, got {year:?}")),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(err) => Err(err).context("cannot read YEAR"),
    }
}

/// Reads `COOKIE` from the environment, accepting both `session=<token>` and a bare token
fn session_cookie() -> anyhow::Result<String> {
    let raw = std::env::var("COOKIE").context(
        "COOKIE is not set, add `COOKIE=session=<token>` to the .env file in your AoC root",
    )?;
    let raw = raw.trim();

    Ok(if raw.starts_with("session=") {
        raw.to_owned()
    } else {
        format!("session={raw}")
    })
}

fn init_puzzle(workspace: &Workspace, puzzle: &Puzzle) -> anyhow::Result<()> {
    let path = workspace.solution_path(puzzle);
    let dir = path.parent().expect("solution path has a year directory");
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;

    match std::fs::File::create_new(&path) {
        Ok(mut file) => {
            file.write_all(SOLUTION_TEMPLATE.as_bytes())
                .context("cannot write to solution file")?;
            println!("Created {}", path.display());
        }
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            println!("{} already exists", path.display());
        }
        Err(err) => return Err(err).context("cannot create solution file"),
    }

    Ok(())
}

fn run_puzzle(workspace: &Workspace, puzzle: &Puzzle) -> anyhow::Result<()> {
    let solution = workspace.solution_path(puzzle);
    if !solution.exists() {
        bail!(
            "{} not found, run `aoc init {} -y {}` to create it",
            solution.display(),
            puzzle.day,
            puzzle.year
        );
    }

    // Some solutions have their input hardcoded, so a missing input file isn't fatal
    let input_path = workspace.input_path(puzzle);
    let input = match std::fs::read(&input_path) {
        Ok(input) => input,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            eprintln!(
                "warning: {} not found (run `aoc get`), running with empty stdin",
                input_path.display()
            );
            Vec::new()
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", input_path.display()));
        }
    };

    let mut child = Command::new("ndc")
        .arg(&solution)
        .current_dir(solution.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to start ndc, is it installed and on your PATH?")?;

    // stdout/stderr are inherited, so output streams straight to the terminal and the child can't block on us.
    // The stdin handle is dropped at the end of this block, which signals EOF.
    {
        let mut stdin = child.stdin.take().context("cannot open ndc stdin")?;
        match stdin.write_all(&input) {
            // The script exited (or doesn't read its input at all) before consuming everything
            Err(err) if err.kind() == ErrorKind::BrokenPipe => {}
            result => result.context("failed to pipe input to ndc")?,
        }
    }

    let status = child.wait().context("failed to wait for ndc")?;
    if !status.success() {
        bail!("ndc exited with {status}");
    }

    Ok(())
}

fn print_puzzle(workspace: &Workspace, puzzle: &Puzzle) -> anyhow::Result<()> {
    print!("{}", workspace.read_input(puzzle)?);
    std::io::stdout().flush().context("failed to flush stdout")
}

fn preview_puzzle(workspace: &Workspace, puzzle: &Puzzle) -> anyhow::Result<()> {
    const HEAD: usize = 20;
    const TAIL: usize = 20;
    const TRUNCATE_ABOVE: usize = 50;

    let text = workspace.read_input(puzzle)?;
    let lines: Vec<&str> = text.lines().collect();

    if lines.is_empty() {
        println!("Input is empty");
        return Ok(());
    }

    // Wide enough for the largest line number and the ".." marker
    let gutter = lines.len().to_string().len().max(2);
    let term_width = terminal_size::terminal_size()
        .map(|(width, _)| usize::from(width.0))
        .unwrap_or(usize::MAX);
    // Space left for line content after "<nr> | "
    let max_content = term_width.saturating_sub(gutter + 3).max(10);

    let print_line = |nr: usize, line: &str| {
        let shown = if line.chars().count() > max_content {
            Cow::Owned(
                line.chars()
                    .take(max_content - 2)
                    .chain("..".chars())
                    .collect::<String>(),
            )
        } else {
            Cow::Borrowed(line)
        };
        println!("{nr:>gutter$} | {shown}");
    };

    if lines.len() > TRUNCATE_ABOVE {
        let tail_start = lines.len() - TAIL;
        for (idx, line) in lines[..HEAD].iter().enumerate() {
            print_line(idx + 1, line);
        }
        let hidden = tail_start - HEAD;
        println!("{:>gutter$} | ...", "..");
        println!("{:>gutter$} | <<{hidden} lines truncated>>", "..");
        println!("{:>gutter$} | ...", "..");
        for (idx, line) in lines[tail_start..].iter().enumerate() {
            print_line(tail_start + idx + 1, line);
        }
    } else {
        for (idx, line) in lines.iter().enumerate() {
            print_line(idx + 1, line);
        }
    }

    let mut lengths: Vec<usize> = lines.iter().map(|line| line.chars().count()).collect();
    lengths.sort_unstable();
    let n = lengths.len();
    let median = if n.is_multiple_of(2) {
        (lengths[n / 2 - 1] + lengths[n / 2]) as f64 / 2.0
    } else {
        lengths[n / 2] as f64
    };

    println!("\nLines: {n}");
    println!("Max line length: {}", lengths[n - 1]);
    println!("Median line length: {median}");
    println!("Min line length: {}", lengths[0]);

    Ok(())
}

fn get_puzzle(workspace: &Workspace, puzzle: &Puzzle, cookie: &str) -> anyhow::Result<()> {
    let output_file = workspace.input_path(puzzle);
    if output_file.exists() {
        return Ok(());
    }

    let unlocks_at = puzzle.unlocks_at();
    let now = Utc::now();
    if now < unlocks_at {
        let wait = unlocks_at.signed_duration_since(now);
        bail!(
            "{} day {} unlocks at {} (in {}h{:02}m)",
            puzzle.year,
            puzzle.day,
            unlocks_at.with_timezone(&Local).format("%Y-%m-%d %H:%M"),
            wait.num_hours(),
            wait.num_minutes() % 60
        );
    }

    let url = format!(
        "https://adventofcode.com/{}/day/{}/input",
        puzzle.year, puzzle.day
    );
    let response = ureq::get(&url)
        .header("Cookie", cookie)
        .header("User-Agent", USER_AGENT)
        .call();

    let mut response = match response {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(400)) => {
            bail!(
                "server rejected the request (HTTP 400), the session cookie is probably missing or expired"
            )
        }
        Err(ureq::Error::StatusCode(404)) => {
            bail!("puzzle input not found (HTTP 404), is the puzzle unlocked yet?")
        }
        Err(ureq::Error::StatusCode(500)) => {
            bail!(
                "server error (HTTP 500), adventofcode.com also responds this way to an invalid session cookie"
            )
        }
        Err(ureq::Error::StatusCode(code)) => bail!("server responded with HTTP {code}"),
        Err(err) => return Err(anyhow!(err)).context("request to adventofcode.com failed"),
    };

    let text = response
        .body_mut()
        .read_to_string()
        .context("cannot read response body")?;

    let dir = output_file
        .parent()
        .expect("input path has a year directory");
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    std::fs::write(&output_file, text)
        .with_context(|| format!("cannot write {}", output_file.display()))?;

    Ok(())
}

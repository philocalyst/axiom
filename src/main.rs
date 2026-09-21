use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;

use axiom_ledger::render::{
    has_blocking_state, journal_is_blocked, render_check, render_journal, render_packages,
    render_why,
};
use axiom_ledger::{analyze, parse_ledger};

const USAGE: &str = "usage: axiom <check|journal|why|packages> FILE [GOAL]";
const HELP: &str = "Axiom — a small, local-first economic ledger\n\n\
usage:\n  axiom check FILE\n  axiom journal FILE\n  axiom why FILE GOAL\n  axiom packages FILE\n\n\
commands:\n  check      inspect source state and unresolved decisions\n  journal    show the derived journal, when it is complete\n  why        explain a goal in ordinary source and policy language\n  packages   list the fixed V0 vocabulary and policy packages\n\n\
Use `axiom --help` for this message.\n";

fn main() {
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let mut stderr = io::BufWriter::new(io::stderr().lock());
    let code = run_with(std::env::args_os(), &mut stdout, &mut stderr);
    let _ = stdout.flush();
    let _ = stderr.flush();
    std::process::exit(code);
}

/// Run the command line with caller-provided streams.  Keeping this small
/// function public makes the CLI easy to exercise without spawning a process.
pub fn run<I, S>(args: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    run_with(args, &mut stdout, &mut stderr)
}

pub fn run_with<I, S, O, E>(args: I, stdout: &mut O, stderr: &mut E) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
    O: Write,
    E: Write,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    let command = match Command::parse(&args) {
        Ok(command) => command,
        Err(message) => {
            let _ = writeln!(stderr, "error: {message}\n{USAGE}");
            return 2;
        }
    };

    if matches!(command.kind, CommandKind::Help) {
        let _ = write!(stdout, "{HELP}");
        return 0;
    }

    let source = match std::fs::read_to_string(&command.path) {
        Ok(source) => source,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "error: cannot read {}: {error}",
                command.path.display()
            );
            return 1;
        }
    };
    let ledger = match parse_ledger(&source) {
        Ok(ledger) => ledger,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "error: {}:{}:{}: {}",
                command.path.display(),
                error.location.line,
                error.location.column,
                error.message
            );
            return 1;
        }
    };
    let analysis = analyze(&ledger);

    let output = match &command.kind {
        CommandKind::Check => render_check(&analysis),
        CommandKind::Journal => render_journal(&analysis),
        CommandKind::Why(goal) => match render_why(&analysis, goal) {
            Ok(output) => output,
            Err(error) => {
                let _ = writeln!(stderr, "error: {error}");
                return 1;
            }
        },
        CommandKind::Packages => render_packages(&analysis.book, analysis.policy.as_deref()),
        CommandKind::Help => unreachable!("help returned before reading a ledger"),
    };
    let _ = write!(stdout, "{output}");

    match command.kind {
        CommandKind::Check => i32::from(has_blocking_state(&analysis)),
        CommandKind::Journal => i32::from(journal_is_blocked(&analysis)),
        CommandKind::Why(_) | CommandKind::Packages => 0,
        CommandKind::Help => 0,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Command {
    kind: CommandKind,
    path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CommandKind {
    Help,
    Check,
    Journal,
    Why(String),
    Packages,
}

impl Command {
    fn parse(args: &[OsString]) -> Result<Self, String> {
        if args
            .iter()
            .skip(1)
            .any(|value| matches!(value.to_str(), Some("-h" | "--help")))
        {
            return Ok(Self {
                kind: CommandKind::Help,
                path: PathBuf::new(),
            });
        }
        let command = args
            .get(1)
            .and_then(|value| value.to_str())
            .ok_or_else(|| "missing command".to_owned())?;
        let kind = match command {
            "help" => {
                if args.len() != 2 {
                    return Err("help does not take arguments".into());
                }
                CommandKind::Help
            }
            "check" => CommandKind::Check,
            "journal" => CommandKind::Journal,
            "packages" => CommandKind::Packages,
            "why" => {
                if args.len() != 4 {
                    return Err("why requires FILE and GOAL (for example `gain:sell`)".into());
                }
                let goal = args[3]
                    .to_str()
                    .ok_or_else(|| "goal is not valid UTF-8".to_owned())?;
                if goal.trim().is_empty() {
                    return Err("why requires a non-empty GOAL".into());
                }
                CommandKind::Why(goal.to_owned())
            }
            other => return Err(format!("unknown command `{other}`")),
        };
        if matches!(kind, CommandKind::Help) {
            return Ok(Self {
                kind,
                path: PathBuf::new(),
            });
        }
        if args.len() != 3 && !matches!(kind, CommandKind::Why(_)) {
            return Err(format!("{command} requires FILE"));
        }
        let path_index = 2;
        let path = args
            .get(path_index)
            .ok_or_else(|| "missing FILE".to_owned())?;
        Ok(Self {
            kind,
            path: PathBuf::from(path),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands_without_accepting_extra_arguments() {
        let args = vec![
            OsString::from("axiom"),
            OsString::from("check"),
            OsString::from("book.axm"),
        ];
        assert_eq!(Command::parse(&args).unwrap().kind, CommandKind::Check);
        let too_many = vec![
            OsString::from("axiom"),
            OsString::from("check"),
            OsString::from("book.axm"),
            OsString::from("extra"),
        ];
        assert!(Command::parse(&too_many).is_err());
    }

    #[test]
    fn why_requires_a_goal() {
        let args = vec![
            OsString::from("axiom"),
            OsString::from("why"),
            OsString::from("book.axm"),
        ];
        assert!(Command::parse(&args).is_err());
    }

    #[test]
    fn help_is_available_without_a_ledger() {
        for flag in ["help", "-h", "--help"] {
            let args = vec![OsString::from("axiom"), OsString::from(flag)];
            assert_eq!(Command::parse(&args).unwrap().kind, CommandKind::Help);
        }
    }

    #[test]
    fn help_prints_without_opening_a_file() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_with(
            [OsString::from("axiom"), OsString::from("--help")],
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(code, 0);
        assert!(
            String::from_utf8(stdout)
                .unwrap()
                .contains("axiom check FILE")
        );
        assert!(stderr.is_empty());
    }
}

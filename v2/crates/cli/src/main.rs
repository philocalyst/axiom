//! `axiom`: the command line.
//!
//! Exit status: 0 when all is well, 1 when the ledger has errors, and 2 when
//! the command could not be run at all (a usage mistake, or no project to run
//! it on).
//!
//! One module per job: `args` reads the command line, `commands` runs a
//! command, `project` finds and reads sources, `render` draws diagnostics,
//! `table` draws reports, `sync` runs the sync scripts, `help` prints the usage
//! screen, `style` knows about colour, and `text` about prose.

mod args;
mod commands;
mod help;
mod project;
mod render;
mod style;
mod sync;
mod table;
mod text;

#[cfg(test)]
mod testing;

use std::io::{self, Write};
use std::process::ExitCode;

use axiom_core::Diagnostic;

use crate::project::Sources;
use crate::render::Renderer;
use crate::style::{ColorChoice, Terminal};

/// What a command produced. The answer goes to standard output and the
/// diagnostics to standard error, so `axiom balance > out.txt` holds only the
/// balances.
pub struct Outcome {
    pub answer: String,
    pub diagnostics: String,
    /// The ledger has errors.
    pub failed: bool,
}

impl Outcome {
    pub fn ok(answer: String) -> Outcome {
        Outcome { answer, diagnostics: String::new(), failed: false }
    }
}

/// Where each kind of output goes, and how it may be drawn there.
#[derive(Clone, Copy)]
pub struct Terminals {
    /// Standard output: answers.
    pub out: Terminal,
    /// Standard error: diagnostics.
    pub err: Terminal,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os().skip(1).map(|arg| arg.to_string_lossy().into_owned()).collect();
    let json = args.iter().any(|argument| argument == "--json" || argument.starts_with("--json="));
    let invocation = match args::parse(&args) {
        Ok(invocation) => invocation,
        Err(usage) => return refuse(&usage, ColorChoice::Auto, json),
    };
    let color = invocation.color;
    let terminals =
        Terminals { out: Terminal::detect(color, &io::stdout()), err: Terminal::detect(color, &io::stderr()) };
    match commands::run(&invocation, terminals) {
        Ok(outcome) => {
            // A closed pipe (`axiom balance | head`) is the reader's choice, not a failure.
            let _ = io::stderr().write_all(outcome.diagnostics.as_bytes());
            let _ = io::stdout().write_all(outcome.answer.as_bytes());
            if outcome.failed { ExitCode::from(1) } else { ExitCode::SUCCESS }
        }
        Err(problem) => refuse(&problem, invocation.color, invocation.json),
    }
}

/// Shows why nothing could be run and exits with 2; JSON mode writes to stdout.
fn refuse(problem: &Diagnostic, color: ColorChoice, json: bool) -> ExitCode {
    if json {
        let output = axiom_report::json::diagnostics(&[problem], &Sources::default());
        let _ = io::stdout().write_all(output.as_bytes());
        return ExitCode::from(2);
    }
    let terminal = Terminal::detect(color, &io::stderr());
    let text = Renderer::new(&Sources::default(), terminal).diagnostic(problem);
    let _ = io::stderr().write_all(text.as_bytes());
    ExitCode::from(2)
}

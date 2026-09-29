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

/// What a command produced: text for standard output, and whether it found
/// errors.
pub struct Outcome {
    pub text: String,
    pub failed: bool,
}

impl Outcome {
    pub fn ok(text: String) -> Outcome {
        Outcome { text, failed: false }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os().skip(1).map(|arg| arg.to_string_lossy().into_owned()).collect();
    let invocation = match args::parse(&args) {
        Ok(invocation) => invocation,
        Err(usage) => return refuse(&usage, ColorChoice::Auto),
    };
    let terminal = Terminal::detect(invocation.global.color, &io::stdout());
    match commands::run(&invocation, terminal) {
        Ok(outcome) => {
            // A closed pipe (`axiom balance | head`) is the reader's choice, not a failure.
            let _ = io::stdout().write_all(outcome.text.as_bytes());
            if outcome.failed { ExitCode::from(1) } else { ExitCode::SUCCESS }
        }
        Err(problem) => refuse(&problem, invocation.global.color),
    }
}

/// Shows why nothing could be run, on standard error, and exits with 2.
fn refuse(problem: &Diagnostic, color: ColorChoice) -> ExitCode {
    let terminal = Terminal::detect(color, &io::stderr());
    let text = Renderer::new(&Sources::default(), terminal).diagnostic(problem);
    let _ = io::stderr().write_all(text.as_bytes());
    ExitCode::from(2)
}

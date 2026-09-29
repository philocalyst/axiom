//! What each command does: load the project, build the book, run it, and show
//! what was asked for.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_core::{Day, Diagnostic};
use axiom_engine::{Options, Run};
use axiom_model::Book;
use axiom_report::{Query, Summary};

use crate::args::{Action, Command, Global, Invocation};
use crate::project::Project;
use crate::render::{Renderer, Tally};
use crate::style::{Ink, Line};
use crate::text::plural;
use crate::{Outcome, Terminals, help, sync, table};

/// Runs the command. An `Err` means there was nothing to run it on: the
/// project could not be found or read.
pub fn run(invocation: &Invocation, terminals: Terminals) -> Result<Outcome, Diagnostic> {
    match &invocation.command {
        Command::Help => Ok(Outcome::ok(help::screen(terminals.out))),
        Command::Version => Ok(Outcome::ok(help::version())),
        Command::Project(action) => run_action(&invocation.global, action, terminals),
    }
}

fn run_action(global: &Global, action: &Action, terminals: Terminals) -> Result<Outcome, Diagnostic> {
    let project = Project::find(global.project.unwrap_or(Path::new(".")))?;
    let sources = project.load()?;
    let (parsed, mut diagnostics) = sources.parse();
    let (book, built) = axiom_model::build(&parsed);
    // The syntax trees are done with; the book borrows only the source text.
    drop(parsed);
    diagnostics.extend(built);

    let options = Options { today: global.today.unwrap_or_else(system_today), relaxed: global.relaxed };
    let renderer = Renderer::new(&sources, terminals.err);
    match action {
        Action::Sync { files } => sync::execute(&book, files, &project.root, terminals.out),
        Action::Check => {
            let run = axiom_engine::run(&book, options);
            Ok(Session::new(&book, &run, &diagnostics, renderer, terminals).check())
        }
        Action::Report(query) => {
            let run = axiom_engine::run(&book, options);
            Ok(Session::new(&book, &run, &diagnostics, renderer, terminals).report(query, global.relaxed))
        }
    }
}

/// The current day, by the system clock, in UTC.
fn system_today() -> Day {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    Day((seconds / 86_400) as i32)
}

/// A book that has been run, and everything found on the way.
struct Session<'a, 's> {
    book: &'a Book<'s>,
    run: &'a Run,
    /// From parsing, building, and running.
    diagnostics: Vec<&'a Diagnostic>,
    renderer: Renderer<'a>,
    terminals: Terminals,
}

impl<'a, 's> Session<'a, 's> {
    fn new(
        book: &'a Book<'s>,
        run: &'a Run,
        earlier: &'a [Diagnostic],
        renderer: Renderer<'a>,
        terminals: Terminals,
    ) -> Session<'a, 's> {
        let diagnostics = earlier.iter().chain(&run.diagnostics).collect();
        Session { book, run, diagnostics, renderer, terminals }
    }

    /// Every diagnostic; and if none is an error, the book in one line.
    fn check(mut self) -> Outcome {
        let found = std::mem::take(&mut self.diagnostics);
        let (diagnostics, tally) = self.show(&found);
        if tally.errors > 0 {
            return Outcome { answer: String::new(), diagnostics, failed: true };
        }
        let summary = axiom_report::summary(self.book, self.run);
        let answer = summary_line(self.book, &summary).render(self.terminals.out.painter) + "\n";
        Outcome { answer, diagnostics, failed: false }
    }

    /// The errors, and unless there are any (and `relaxed` does not say to
    /// carry on regardless) the report.
    fn report(mut self, query: &Query, relaxed: bool) -> Outcome {
        let errors: Vec<&Diagnostic> =
            self.diagnostics.iter().copied().filter(|diagnostic| diagnostic.is_error()).collect();
        let (mut diagnostics, tally) = self.show(&errors);
        if tally.errors > 0 && !relaxed {
            return Outcome { answer: String::new(), diagnostics, failed: true };
        }
        match axiom_report::report(self.book, self.run, query) {
            Ok(report) => {
                let answer = table::render(&report, self.terminals.out);
                Outcome { answer, diagnostics, failed: tally.errors > 0 }
            }
            Err(refusal) => {
                diagnostics.push_str(&self.show(&[&refusal]).0);
                Outcome { answer: String::new(), diagnostics, failed: true }
            }
        }
    }

    /// The diagnostics, and after them how many of each there were.
    fn show(&mut self, diagnostics: &[&Diagnostic]) -> (String, Tally) {
        let mut text = self.renderer.diagnostics(diagnostics);
        let tally = Tally::of(diagnostics);
        if let Some(line) = tally.line() {
            text.push_str(&line.render(self.terminals.err.painter));
            text.push('\n');
        }
        (text, tally)
    }
}

/// `✓ 1,284 flows · 23 places · 4 laws enforced · net worth 184,220.13 USD`
fn summary_line(book: &Book, summary: &Summary) -> Line {
    let facts = [
        plural(summary.flows, "flow"),
        plural(summary.places, "place"),
        format!("{} enforced", plural(summary.laws, "law")),
        format!("net worth {}", book.show(summary.net_worth)),
    ];
    let mut line = Line::text("✓ ", Ink::GREEN.bold());
    for (at, fact) in facts.iter().enumerate() {
        if at > 0 {
            line.push(" · ", Ink::DIM);
        }
        line.push(fact, Ink::PLAIN);
    }
    if summary.unpriced > 0 {
        line.push(" · ", Ink::DIM);
        line.push(&format!("{} unpriced", plural(summary.unpriced, "holding")), Ink::YELLOW);
    }
    line
}

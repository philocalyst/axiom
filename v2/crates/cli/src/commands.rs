//! What each command does: load the project, build the book, run it, and show
//! what was asked for.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_core::{Day, Diagnostic};
use axiom_engine::{Options, Run};
use axiom_model::Book;
use axiom_report::{Query, Summary};

use crate::Outcome;
use crate::args::{Action, Command, Global, Invocation};
use crate::project::Project;
use crate::render::{Renderer, Tally};
use crate::style::{Ink, Line, Terminal};
use crate::text::plural;
use crate::{help, sync, table};

/// Runs the command. An `Err` means there was nothing to run it on: the
/// project could not be found or read.
pub fn run(invocation: &Invocation, terminal: Terminal) -> Result<Outcome, Diagnostic> {
    match &invocation.command {
        Command::Help => Ok(Outcome::ok(help::screen(terminal))),
        Command::Version => Ok(Outcome::ok(help::version())),
        Command::Project(action) => run_action(&invocation.global, action, terminal),
    }
}

fn run_action(global: &Global, action: &Action, terminal: Terminal) -> Result<Outcome, Diagnostic> {
    let project = Project::find(global.project.unwrap_or(Path::new(".")))?;
    let sources = project.load()?;
    let (parsed, mut diagnostics) = sources.parse();
    let (book, built) = axiom_model::build(&parsed);
    // The syntax trees are done with; the book borrows only the source text.
    drop(parsed);
    diagnostics.extend(built);

    let options = Options { today: global.today.unwrap_or_else(system_today), relaxed: global.relaxed };
    let renderer = Renderer::new(&sources, terminal);
    match action {
        Action::Sync { files } => sync::execute(&book, files, &project.root, terminal),
        Action::Check => {
            let run = axiom_engine::run(&book, options);
            Ok(Session::new(&book, &run, &diagnostics, renderer, terminal).check())
        }
        Action::Report(query) => {
            let run = axiom_engine::run(&book, options);
            Ok(Session::new(&book, &run, &diagnostics, renderer, terminal).report(query, global.relaxed))
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
    terminal: Terminal,
}

impl<'a, 's> Session<'a, 's> {
    fn new(
        book: &'a Book<'s>,
        run: &'a Run,
        earlier: &'a [Diagnostic],
        renderer: Renderer<'a>,
        terminal: Terminal,
    ) -> Session<'a, 's> {
        let diagnostics = earlier.iter().chain(&run.diagnostics).collect();
        Session { book, run, diagnostics, renderer, terminal }
    }

    /// Every diagnostic; and if none is an error, the book in one line.
    fn check(mut self) -> Outcome {
        let (mut text, tally) = show(&mut self.renderer, self.terminal, &self.diagnostics);
        if tally.errors > 0 {
            return Outcome { text, failed: true };
        }
        let summary = axiom_report::summary(self.book, self.run);
        text.push_str(&summary_line(self.book, &summary).render(self.terminal.painter));
        text.push('\n');
        Outcome::ok(text)
    }

    /// The errors, and unless there are any (and `relaxed` does not say to
    /// carry on regardless) the report.
    fn report(mut self, query: &Query, relaxed: bool) -> Outcome {
        let errors: Vec<&Diagnostic> =
            self.diagnostics.iter().copied().filter(|diagnostic| diagnostic.is_error()).collect();
        let (mut text, tally) = show(&mut self.renderer, self.terminal, &errors);
        if tally.errors > 0 && !relaxed {
            return Outcome { text, failed: true };
        }
        match axiom_report::report(self.book, self.run, query) {
            Ok(report) => text.push_str(&table::render(&report, self.terminal)),
            Err(diagnostic) => {
                text.push_str(&show(&mut self.renderer, self.terminal, &[&diagnostic]).0);
                return Outcome { text, failed: true };
            }
        }
        Outcome { text, failed: tally.errors > 0 }
    }
}

/// The diagnostics, and after them how many of each there were.
fn show(renderer: &mut Renderer, terminal: Terminal, diagnostics: &[&Diagnostic]) -> (String, Tally) {
    let mut text = renderer.diagnostics(diagnostics);
    let tally = Tally::of(diagnostics);
    if let Some(line) = tally.line() {
        text.push_str(&line.render(terminal.painter));
        text.push('\n');
    }
    (text, tally)
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

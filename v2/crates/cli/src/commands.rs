//! What each command does: load the project, build the book, run it, and show
//! what was asked for.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use axiom_core::{Day, Diagnostic};
use axiom_engine::{Options, Run};
use axiom_model::Book;
use axiom_report::{Query, ReportRenderer, Summary, json::JsonRenderer};

use crate::args::{Command, Invocation};
use crate::project::{Project, Sources};
use crate::render::{Renderer, Tally};
use crate::style::{Ink, Line};
use crate::text::plural;
use crate::{Outcome, Terminals, help, sync, table};

/// Runs the command. An `Err` means there was nothing to run it on: the
/// project could not be found or read.
pub fn run(invocation: &Invocation, terminals: Terminals) -> Result<Outcome, Diagnostic> {
    let Invocation { command, .. } = invocation;
    match command {
        Command::Help => return Ok(Outcome::ok(help::screen(terminals.out))),
        Command::Version => return Ok(Outcome::ok(help::version())),
        _ => {}
    }
    let project = Project::find(invocation.project.unwrap_or(Path::new(".")))?;
    let sources = project.load()?;
    let (parsed, mut diagnostics) = sources.parse();
    let (book, built) = axiom_model::build(&parsed);
    // The syntax trees are done with; the book borrows only the source text.
    drop(parsed);
    diagnostics.extend(built);

    if let Command::Sync(files) = command {
        return sync::execute(&book, files, &project.root, terminals.out);
    }
    let options = Options { today: invocation.today.unwrap_or_else(system_today), relaxed: invocation.relaxed };
    let run = axiom_engine::run(&book, options);
    let session = Session {
        book: &book,
        run: &run,
        sources: &sources,
        diagnostics: diagnostics.iter().chain(&run.diagnostics).collect(),
        terminals,
        all: invocation.all,
        json: invocation.json,
    };
    Ok(if let Command::Report(query, whose) = command { session.report(query, *whose) } else { session.check() })
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
    sources: &'a Sources,
    /// From parsing, building, and running.
    diagnostics: Vec<&'a Diagnostic>,
    terminals: Terminals,
    /// Show every diagnostic, however many.
    all: bool,
    /// Machine-readable rendering is independent of terminal width and colour.
    json: bool,
}

impl Session<'_, '_> {
    /// Every diagnostic; and if none is an error, the book in one line.
    fn check(&self) -> Outcome {
        let (diagnostics, tally) = self.show(&self.diagnostics);
        if self.json {
            return Outcome { answer: diagnostics, diagnostics: String::new(), failed: tally.errors > 0 };
        }
        if tally.errors > 0 {
            return Outcome { answer: String::new(), diagnostics, failed: true };
        }
        let summary = axiom_report::summary(self.book, self.run);
        let answer = self.terminals.out.painter.paint(&[summary_line(self.book, &summary)]);
        Outcome { answer, diagnostics, failed: false }
    }

    /// The errors, and the report. A report runs whatever the book's errors, so
    /// that a reader can investigate them; it says at its head what it rests on.
    fn report(&self, query: &Query, whose: Option<&str>) -> Outcome {
        let result = axiom_report::report_with_sources(self.book, self.run, query, whose, self.sources);
        let mut shown: Vec<&Diagnostic> = self.diagnostics.iter().copied().filter(|found| found.is_error()).collect();
        shown.extend(result.as_ref().err());
        let (diagnostics, tally) = self.show(&shown);
        let Ok(report) = result else {
            return if self.json {
                Outcome { answer: diagnostics, diagnostics: String::new(), failed: true }
            } else {
                Outcome { answer: String::new(), diagnostics, failed: true }
            };
        };
        if self.json {
            return Outcome {
                answer: JsonRenderer.render(&report, self.sources),
                diagnostics,
                failed: tally.errors > 0,
            };
        }
        let mut answer = table::TableRenderer { terminal: self.terminals.out }.render(&report, self.sources);
        if tally.errors > 0 {
            let caveat = format!(
                "rests on a book with {} (`axiom check` lists them): what they touch may be wrong",
                plural(tally.errors, "error")
            );
            let mut line = Line::text("✗ ", Ink::RED.bold());
            line.push(&caveat, Ink::DIM);
            answer.insert_str(0, &self.terminals.out.painter.paint(&[line, Line::new()]));
        }
        Outcome { answer, diagnostics, failed: tally.errors > 0 }
    }

    /// The diagnostics, and after them how many of each there were.
    fn show(&self, diagnostics: &[&Diagnostic]) -> (String, Tally) {
        if self.json {
            return (crate::render::json::diagnostics(diagnostics, self.sources), Tally::of(diagnostics.iter().copied()));
        }
        let (mut text, tally) = Renderer::new(self.sources, self.terminals.err).present(diagnostics, self.all);
        if let Some(line) = tally.line() {
            text += &self.terminals.err.painter.paint(&[line]);
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
    line.push(&facts.join(" · "), Ink::PLAIN);
    if summary.unpriced > 0 {
        line.push(&format!(" · {} unpriced", plural(summary.unpriced, "holding")), Ink::YELLOW);
    }
    line
}

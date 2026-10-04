//! `axiom fmt`: lay out only the requested source files using the syntax
//! crate's formatter, or, with `--upgrade`, write the v4 lines of them the v5
//! way first. Formatting is planned for every selected file before a write
//! begins, so a bad path or a source outside the project cannot leave a
//! partially selected set behind.
//!
//! An upgrade needs what no text says (which end of a line is the book's own,
//! and who owns it), so it opens the project as a session and asks the book
//! ([`BookRegistry`]). It then checks its own work: a file's new text is tried
//! on a copy of the session, and kept only if the book finds nothing it did not
//! find before and says what it said.

use std::fs;
use std::path::Path;

use axiom_core::{Diagnostic, Loc};
use axiom_model::{Book, Class};
use axiom_report::Query;
use axiom_session::{Edit, Options, Session, Sources};
use axiom_syntax::{Folder, Registry, Standing};

use crate::Outcome;
use crate::args::{Effect, Format, Rewrite};
use crate::style::{Ink, Line, Terminal};

/// Formats every project source, or only the named sources.
pub fn execute(
    sources: &Sources<'_>,
    root: &Path,
    format: &Format<'_>,
    options: Options,
    terminal: Terminal,
) -> Outcome {
    match plan(sources, format, options) {
        Ok(planned) => apply(sources, root, planned, format.effect, terminal),
        Err(problem) => Outcome {
            answer: String::new(),
            diagnostics: crate::render::Renderer::new(sources, terminal).diagnostic(&problem),
            failed: true,
        },
    }
}

struct Change<'a> {
    path: &'a str,
    output: String,
}

/// What a request comes to: the files that change, and the lines an upgrade would not guess.
struct Planned<'a> {
    changes: Vec<Change<'a>>,
    refused: Vec<Diagnostic>,
}

fn plan<'a>(sources: &'a Sources<'_>, format: &Format<'_>, options: Options) -> Result<Planned<'a>, Diagnostic> {
    let selected = selected(sources, &format.files)?;
    let (parsed, _) = sources.parse();
    let session = (format.rewrite == Rewrite::Upgrade).then(|| Session::open(sources.clone(), options));
    let mut planned = Planned { changes: Vec::new(), refused: Vec::new() };
    for path in selected {
        let source = parsed
            .iter()
            .find(|source| source.path == path)
            .ok_or_else(|| Diagnostic::error("missing-source", format!("could not parse project source `{path}`")))?;
        match &session {
            None => planned.changes.push(Change { path, output: source.file.format() }),
            Some(session) => match upgraded(session, &source.file) {
                Ok(output) => planned.changes.push(Change { path, output }),
                Err(mut lines) => planned.refused.append(&mut lines),
            },
        }
    }
    Ok(planned)
}

/// The project sources `wanted` names, once each, or all of them when none is named.
fn selected<'a>(sources: &'a Sources<'_>, wanted: &[&str]) -> Result<Vec<&'a str>, Diagnostic> {
    let paths: Vec<&str> = sources.project_paths().collect();
    fn normalize(path: &str) -> &str {
        path.strip_prefix("./").unwrap_or(path)
    }
    if let Some(&unknown) = wanted.iter().find(|&&path| !paths.contains(&normalize(path))) {
        let error = Diagnostic::error("no-source", format!("no project source named `{unknown}`"));
        let near = axiom_core::diag::closest(unknown, paths.iter().copied());
        return Err(match near {
            Some(near) => error.help(format!("did you mean `{near}`?")),
            None => error,
        });
    }
    if wanted.is_empty() {
        return Ok(paths);
    }
    let named = wanted.iter().map(|path| {
        let name = normalize(path);
        paths.iter().copied().find(|candidate| *candidate == name).expect("requested source was checked above")
    });
    Ok(named.fold(Vec::new(), |mut selected, path| {
        if !selected.contains(&path) {
            selected.push(path);
        }
        selected
    }))
}

/// `file` written the v5 way, if that changes nothing the book says; otherwise why it is not.
fn upgraded(session: &Session<'_>, file: &axiom_syntax::File<'_>) -> Result<String, Vec<Diagnostic>> {
    let source = session.sources().get(file.id).expect("a parsed file is a source");
    let registry = BookRegistry(session.book());
    let found = axiom_syntax::upgrade(&source.text, file, Folder::of(&source.path), &registry);
    let whole = Edit::Replace { at: Loc::new(file.id, 0, source.text.len() as u32), text: found.text.clone() };
    match session.what_if(&whole, |after| say_the_same(session, after)) {
        Ok(Ok(())) if found.refused.is_empty() => Ok(found.text),
        Ok(Ok(())) => Err(found.refused),
        Ok(Err(changed)) => Err(vec![changed]),
        Err(refused) => Err(vec![Diagnostic::error("upgrade-unparsable", refused.to_string())]),
    }
}

/// Whether `after` says what `before` did: no diagnostic but the v4 warning gone, and the same book by what it counts and
/// every balance.
fn say_the_same(before: &Session<'_>, after: &Session<'_>) -> Result<(), Diagnostic> {
    let (a, b) = (before.summary(), after.summary());
    let balance = Query::Balance { globs: vec![], at: None, value: false, monthly: false };
    let shown = |session: &Session<'_>| session.query(&balance, None).map(|report| report.sections.len());
    let changed =
        |what: &str| Diagnostic::error("upgrade-changes-the-book", format!("the upgrade would change {what}"));
    let (flows, places, worth) = ((a.flows, b.flows), (a.places, b.places), (a.net_worth, b.net_worth));
    match (flows.0 == flows.1, places.0 == places.1, worth.0 == worth.1, shown(before).ok() == shown(after).ok()) {
        (true, true, true, true) => Ok(()),
        (false, ..) => Err(changed("the flows the book holds")),
        (_, false, ..) => Err(changed("the places the book holds")),
        _ => Err(changed("what the book is worth")),
    }
}

/// A book's answers to what an upgrade asks.
struct BookRegistry<'b, 's>(&'b Book<'s>);

impl Registry for BookRegistry<'_, '_> {
    fn standing(&self, name: &str) -> Standing {
        let book = self.0;
        let class = |place| match book.places[place].class {
            Class::Outside => Standing::Outside,
            Class::Asset | Class::Debt => Standing::Own,
        };
        if name == "?" || name.starts_with(|c: char| c.is_ascii_uppercase()) {
            return Standing::Outside;
        }
        if let Some(contract) = book.contract(name) {
            return if book.contracts[contract].loan.is_some() { Standing::Own } else { Standing::Outside };
        }
        let entity = book.entity(name).ok().and_then(|entity| book.entities[entity].place);
        match entity.or_else(|| book.place(name).ok()) {
            Some(place) => class(place),
            None => Standing::Unknown,
        }
    }

    fn owner(&self, name: &str) -> Option<&str> {
        let book = self.0;
        let place = book.place(name).ok()?;
        Some(book.name(book.entities[book.places[place].owner].path))
    }

    fn keeper(&self) -> &str {
        self.0.name(self.0.entities[self.0.roots.me].path)
    }

    fn base(&self) -> &str {
        self.0.name(self.0.commodities[self.0.base].symbol)
    }

    fn scale(&self, unit: &str) -> Option<u8> {
        self.0.commodity(unit).map(|unit| self.0.commodities[unit].scale)
    }
}

fn apply(sources: &Sources<'_>, root: &Path, planned: Planned<'_>, effect: Effect, terminal: Terminal) -> Outcome {
    let Planned { changes, refused } = planned;
    let renderer = crate::render::Renderer::new(sources, terminal);
    let mut changed = Vec::new();
    for change in changes {
        let Some(source) = sources.find(change.path) else {
            continue;
        };
        if change.output == source.text {
            continue;
        }
        let target = root.join(change.path);
        if let Err(problem) = ensure_inside(root, &target) {
            return Outcome { answer: String::new(), diagnostics: renderer.diagnostic(&problem), failed: true };
        }
        changed.push((change.path, target, change.output));
    }
    let said: String = refused.iter().map(|line| renderer.diagnostic(line)).collect();
    let failed = !refused.is_empty();

    if effect == Effect::Check {
        if changed.is_empty() && !failed {
            return Outcome::ok(terminal.painter.paint(&[Line::text("all selected files are formatted", Ink::GREEN)]));
        }
        let lines: Vec<_> =
            changed.iter().map(|(path, _, _)| Line::text(&format!("would format {path}"), Ink::YELLOW)).collect();
        return Outcome { answer: terminal.painter.paint(&lines), diagnostics: said, failed: true };
    }

    for (_, target, output) in &changed {
        if let Err(error) = fs::write(target, output) {
            let problem = Diagnostic::error("write-failed", format!("could not format {}: {error}", target.display()));
            return Outcome { answer: String::new(), diagnostics: renderer.diagnostic(&problem), failed: true };
        }
    }
    let lines: Vec<_> = match changed.is_empty() {
        true => vec![Line::text("all selected files are formatted", Ink::GREEN)],
        false => changed.iter().map(|(path, _, _)| Line::text(&format!("formatted {path}"), Ink::GREEN)).collect(),
    };
    Outcome { answer: terminal.painter.paint(&lines), diagnostics: said, failed }
}

fn ensure_inside(root: &Path, target: &Path) -> Result<(), Diagnostic> {
    let canonical = fs::canonicalize(target)
        .map_err(|error| Diagnostic::error("write-failed", format!("cannot open {}: {error}", target.display())))?;
    if !canonical.starts_with(root) {
        return Err(Diagnostic::error(
            "outside-project",
            format!("refusing to format `{}` because it resolves outside the project", target.display()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use axiom_core::Day;
    use axiom_session::Texts;

    use super::*;
    use crate::project::Project;
    use crate::style::Terminal;
    use crate::testing::TempDir;

    fn options() -> Options {
        Options { today: Day::from_ymd(2026, 6, 30).unwrap(), relaxed: false }
    }

    fn layout<'a>(files: &[&'a str], effect: Effect) -> Format<'a> {
        Format { files: files.to_vec(), rewrite: Rewrite::Layout, effect }
    }

    #[test]
    fn check_is_idempotent_and_only_targets_named_project_files() {
        let dir = TempDir::new("fmt");
        dir.write("axiom.ax", "base USD\n");
        let source = "2026-01-05   checking   12.5 USD   ->   food\n";
        let (file, diagnostics) =
            axiom_syntax::parse(axiom_core::FileId(0), source, axiom_syntax::Folder::of("journal/2026.ax"));
        assert!(diagnostics.is_empty(), "fixture must parse as native v4 syntax");
        assert_ne!(file.format(), source, "the formatter must change this input");
        assert_eq!(file.format(), "2026-01-05 checking 12.5 USD -> food\n");
        dir.write("journal/2026.ax", source);
        let project = Project::find(dir.path()).unwrap();
        let texts = Texts::default();
        let sources = project.load(&texts).unwrap();
        let check = layout(&["journal/2026.ax"], Effect::Check);
        let checked = execute(&sources, &project.root, &check, options(), Terminal::plain(80));
        assert!(checked.failed, "unformatted selected file is reported");
        assert_eq!(fs::read_to_string(dir.path().join("axiom.ax")).unwrap(), "base USD\n");
        assert_eq!(fs::read_to_string(dir.path().join("journal/2026.ax")).unwrap(), source);

        let write = layout(&["journal/2026.ax"], Effect::Write);
        let formatted = execute(&sources, &project.root, &write, options(), Terminal::plain(80));
        assert!(!formatted.failed);
        let first = fs::read_to_string(dir.path().join("journal/2026.ax")).unwrap();
        assert_eq!(first, "2026-01-05 checking 12.5 USD -> food\n");
        let sources = project.load(&texts).unwrap();
        let checked = execute(&sources, &project.root, &check, options(), Terminal::plain(80));
        assert!(!checked.failed);
        assert_eq!(fs::read_to_string(dir.path().join("journal/2026.ax")).unwrap(), first);
        assert_eq!(fs::read_to_string(dir.path().join("axiom.ax")).unwrap(), "base USD\n");
    }

    #[test]
    fn fmt_rejects_unknown_and_out_of_project_file_targets() {
        let dir = TempDir::new("fmt-path");
        dir.write("axiom.ax", "base USD\n");
        let project = Project::find(dir.path()).unwrap();
        let texts = Texts::default();
        let sources = project.load(&texts).unwrap();
        assert!(plan(&sources, &layout(&["missing.ax"], Effect::Write), options()).is_err());
        let outside = dir.path().parent().unwrap().join("elsewhere.ax");
        fs::write(&outside, "base USD\n").unwrap();
        let error = ensure_inside(&project.root, &outside).unwrap_err();
        assert_eq!(error.code, "outside-project");
        let _ = fs::remove_file(outside);
    }
}

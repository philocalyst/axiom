//! `axiom fmt`: lay out only the requested source files using the syntax
//! crate's formatter. Formatting is planned for every selected file before a
//! write begins, so a bad path or a source outside the project cannot leave a
//! partially selected set behind.

use std::fs;
use std::path::Path;

use axiom_core::Diagnostic;

use crate::Outcome;
use crate::project::Sources;
use crate::style::{Ink, Line, Terminal};

/// Formats every project source, or only the named sources. `--check` reports
/// differences and never writes them.
pub fn execute(
    sources: &Sources,
    root: &Path,
    wanted: &[&str],
    check: bool,
    terminal: Terminal,
) -> Outcome {
    match plan(sources, wanted) {
        Ok(changes) => apply(sources, root, changes, check, terminal),
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

fn plan<'a>(sources: &'a Sources, wanted: &[&str]) -> Result<Vec<Change<'a>>, Diagnostic> {
    let paths: Vec<&str> = sources.project_paths().collect();
    let normalize = |path: &str| path.strip_prefix("./").unwrap_or(path);
    if let Some(&unknown) = wanted
        .iter()
        .find(|&&path| !paths.contains(&normalize(path)))
    {
        let error = Diagnostic::error("no-source", format!("no project source named `{unknown}`"));
        let near = axiom_core::diag::closest(unknown, paths.iter().copied());
        return Err(match near {
            Some(near) => error.help(format!("did you mean `{near}`?")),
            None => error,
        });
    }
    let selected: Vec<&str> = if wanted.is_empty() {
        paths
    } else {
        wanted
            .iter()
            .map(|path| normalize(path))
            .fold(Vec::new(), |mut selected, path| {
                if !selected.contains(&path) {
                    selected.push(path);
                }
                selected
            })
    };

    let (parsed, _) = sources.parse();
    selected
        .into_iter()
        .map(|path| {
            let source = parsed
                .iter()
                .find(|source| source.path == path)
                .ok_or_else(|| {
                    Diagnostic::error(
                        "missing-source",
                        format!("could not parse project source `{path}`"),
                    )
                })?;
            Ok(Change {
                path,
                output: source.file.format(),
            })
        })
        .collect()
}

fn apply(
    sources: &Sources,
    root: &Path,
    changes: Vec<Change<'_>>,
    check: bool,
    terminal: Terminal,
) -> Outcome {
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
            return Outcome {
                answer: String::new(),
                diagnostics: crate::render::Renderer::new(sources, terminal).diagnostic(&problem),
                failed: true,
            };
        }
        changed.push((change.path, target, change.output));
    }

    if check {
        if changed.is_empty() {
            return Outcome::ok(
                terminal
                    .painter
                    .paint(&[Line::text("all selected files are formatted", Ink::GREEN)]),
            );
        }
        let lines: Vec<_> = changed
            .iter()
            .map(|(path, _, _)| Line::text(&format!("would format {path}"), Ink::YELLOW))
            .collect();
        return Outcome {
            answer: terminal.painter.paint(&lines),
            diagnostics: String::new(),
            failed: true,
        };
    }

    for (_, target, output) in &changed {
        if let Err(error) = fs::write(target, output) {
            let problem = Diagnostic::error(
                "write-failed",
                format!("could not format {}: {error}", target.display()),
            );
            return Outcome {
                answer: String::new(),
                diagnostics: crate::render::Renderer::new(sources, terminal).diagnostic(&problem),
                failed: true,
            };
        }
    }
    if changed.is_empty() {
        Outcome::ok(
            terminal
                .painter
                .paint(&[Line::text("all selected files are formatted", Ink::GREEN)]),
        )
    } else {
        let lines: Vec<_> = changed
            .iter()
            .map(|(path, _, _)| Line::text(&format!("formatted {path}"), Ink::GREEN))
            .collect();
        Outcome::ok(terminal.painter.paint(&lines))
    }
}

fn ensure_inside(root: &Path, target: &Path) -> Result<(), Diagnostic> {
    let canonical = fs::canonicalize(target).map_err(|error| {
        Diagnostic::error(
            "write-failed",
            format!("cannot open {}: {error}", target.display()),
        )
    })?;
    if !canonical.starts_with(root) {
        return Err(Diagnostic::error(
            "outside-project",
            format!(
                "refusing to format `{}` because it resolves outside the project",
                target.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use crate::style::Terminal;
    use crate::testing::TempDir;

    #[test]
    fn check_is_idempotent_and_only_targets_named_project_files() {
        let dir = TempDir::new("fmt");
        dir.write("axiom.ax", "base USD\n");
        dir.write("journal/2026.ax", "2026-01-05 checking -> food 12.5 USD\n");
        let project = Project::find(dir.path()).unwrap();
        let sources = project.load().unwrap();
        let checked = execute(
            &sources,
            &project.root,
            &["journal/2026.ax"],
            true,
            Terminal::plain(80),
        );
        assert!(checked.failed, "unformatted selected file is reported");
        assert_eq!(
            fs::read_to_string(dir.path().join("axiom.ax")).unwrap(),
            "base USD\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("journal/2026.ax")).unwrap(),
            "2026-01-05 checking -> food 12.5 USD\n"
        );

        let formatted = execute(
            &sources,
            &project.root,
            &["journal/2026.ax"],
            false,
            Terminal::plain(80),
        );
        assert!(!formatted.failed);
        let first = fs::read_to_string(dir.path().join("journal/2026.ax")).unwrap();
        let sources = project.load().unwrap();
        let checked = execute(
            &sources,
            &project.root,
            &["journal/2026.ax"],
            true,
            Terminal::plain(80),
        );
        assert!(!checked.failed);
        assert_eq!(
            fs::read_to_string(dir.path().join("journal/2026.ax")).unwrap(),
            first
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("axiom.ax")).unwrap(),
            "base USD\n"
        );
    }

    #[test]
    fn fmt_rejects_unknown_and_out_of_project_file_targets() {
        let dir = TempDir::new("fmt-path");
        dir.write("axiom.ax", "base USD\n");
        let project = Project::find(dir.path()).unwrap();
        let sources = project.load().unwrap();
        assert!(plan(&sources, &["missing.ax"]).is_err());
        let outside = dir.path().parent().unwrap().join("elsewhere.ax");
        fs::write(&outside, "base USD\n").unwrap();
        let error = ensure_inside(&project.root, &outside).unwrap_err();
        assert_eq!(error.code, "outside-project");
        let _ = fs::remove_file(outside);
    }
}

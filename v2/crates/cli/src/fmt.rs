//! `axiom fmt`: lays files out in the house style, or with `--check` says which
//! are not.

use std::fs;
use std::path::Path;

use axiom_core::Diagnostic;
use axiom_core::diag::closest;
use axiom_syntax::File;

use crate::Outcome;
use crate::project::{SourceFile, Sources};

/// Formats the files named (default: every file of the project) in place. With
/// `check`, nothing is written, and the outcome fails if anything would change.
/// A file that does not parse is left as it is, and fails the outcome too.
pub fn execute(sources: &Sources, root: &Path, files: &[&str], check: bool) -> Result<Outcome, Diagnostic> {
    let chosen: Vec<&SourceFile> = if files.is_empty() {
        sources.own().collect()
    } else {
        files.iter().map(|&name| named(sources, name)).collect::<Result<_, _>>()?
    };
    let (mut answer, mut failed) = (String::new(), false);
    for file in chosen {
        let (tree, found) = axiom_syntax::parse(file.id, &file.text);
        if found.iter().any(Diagnostic::is_error) {
            answer += &format!("{}: does not parse, left as it is\n", file.path);
            failed = true;
            continue;
        }
        let formatted = format(&file.text, &tree);
        if formatted == *file.text {
            continue;
        }
        if check {
            answer += &format!("{} is not in the house style\n", file.path);
            failed = true;
        } else {
            fs::write(root.join(&*file.path), formatted)
                .map_err(|error| Diagnostic::error("unwritable", format!("cannot write {}: {error}", file.path)))?;
            answer += &format!("formatted {}\n", file.path);
        }
    }
    Ok(Outcome { answer, diagnostics: String::new(), failed })
}

/// The project's file called `name`, or the error that suggests the one meant.
fn named<'a>(sources: &'a Sources, name: &str) -> Result<&'a SourceFile, Diagnostic> {
    sources.own().find(|file| file.path == name).ok_or_else(|| {
        let error = Diagnostic::error("unknown-file", format!("no source file named `{name}`"));
        match closest(name, sources.own().map(|file| &*file.path)) {
            Some(near) => error.help(format!("did you mean `{near}`?")),
            None => error,
        }
    })
}

// Stub until lane S5's `axiom_syntax::format` lands: what is written is the style.
fn format(text: &str, _tree: &File) -> String {
    text.to_string()
}

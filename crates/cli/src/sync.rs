//! The CLI adapter for model-native sync planning.
//!
//! Planning registers every input and generated text in the same append-only
//! catalog that owns diagnostic source IDs. It does not write project files;
//! command dispatch renders the returned plan and applies changes when asked.

use axiom_core::{Day, Diagnostic, FileId};
use axiom_engine::Run;
use axiom_model::Book;
use axiom_session::Sources;
use axiom_sync::{PlanOutcome, SourceRegistry};

use crate::project::Project;

/// Plan the selected native sync sources without writing project files.
///
/// `sources` holds the Axiom files `book` was built from, and the planning adds
/// to it what the sources read and the commands print, so the returned
/// diagnostic IDs are real IDs in it.
pub fn plan(
    book: &Book<'_>,
    run: &Run,
    project: &Project,
    today: Day,
    wanted: &[&str],
    sources: &mut Sources<'_>,
) -> Result<PlanOutcome, Diagnostic> {
    let project_paths: Vec<&str> = sources.project_paths().collect();
    let file_paths: Vec<&str> = sources.files().map(|file| &*file.path).collect();
    let mut registry = Catalog { project, sources };
    axiom_sync::plan(book, run, &project.root, today, wanted, &project_paths, &file_paths, &mut registry)
}

struct Catalog<'a, 't> {
    project: &'a Project,
    sources: &'a mut Sources<'t>,
}

impl SourceRegistry for Catalog<'_, '_> {
    fn read(&mut self, path: &str) -> Result<Option<FileId>, Diagnostic> {
        // A declared reader may name an Axiom source already loaded by the
        // project. Reuse that exact text and identity instead of reading or
        // registering a duplicate.
        if let Some(source) = self.sources.files().find(|source| !source.embedded && source.path == path) {
            return Ok(Some(source.id));
        }
        let text = self.project.read_local(path)?;
        let file = self.sources.append_auxiliary(path.to_owned(), text)?;
        Ok(Some(file))
    }

    fn text(&self, file: FileId) -> Option<&str> {
        self.sources.get(file).map(|source| source.text.as_ref())
    }

    fn generated(&mut self, path: &str, text: String) -> Result<FileId, Diagnostic> {
        self.sources.append_auxiliary(path.to_owned(), text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;
    use axiom_session::Texts;

    #[test]
    fn local_reader_and_generated_text_receive_real_append_only_ids() {
        let dir = TempDir::new("sync-catalog");
        dir.write("axiom.ax", "base USD\n");
        dir.write("statement.csv", "date,amount,memo\n");
        let project = Project::find(dir.path()).unwrap();
        let texts = Texts::default();
        let mut sources = project.load(&texts).unwrap();
        let mut catalog = Catalog { project: &project, sources: &mut sources };

        let input = catalog.read("statement.csv").unwrap().unwrap();
        assert_eq!(catalog.text(input), Some("date,amount,memo\n"));
        let generated = catalog.generated("out.ax", "2026-01-01 a -> b 1 USD\n".to_owned()).unwrap();
        assert_eq!(generated.0, input.0 + 1);
        assert_eq!(catalog.text(generated), Some("2026-01-01 a -> b 1 USD\n"));
    }
}

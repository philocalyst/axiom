//! The CLI adapter for model-native sync planning.
//!
//! Planning registers every input and generated text in the same append-only
//! catalog that owns diagnostic source IDs. It does not write project files;
//! command dispatch renders the returned plan and applies changes when asked.

use axiom_core::{Day, Diagnostic, FileId};
use axiom_engine::Run;
use axiom_model::Book;
use axiom_sync::{PlanOutcome, SourceRegistry};

use crate::project::{Project, SourceFile, Sources};

/// Plan the selected native sync sources without writing project files.
///
/// `files` is the immutable Axiom source catalog borrowed by `book`;
/// `auxiliary` is a disjoint append-only catalog for statement inputs and
/// generated command output. The returned diagnostic IDs are real IDs in the
/// combined source catalog.
pub fn plan(
    book: &Book<'_>,
    run: &Run,
    project: &Project,
    today: Day,
    wanted: &[&str],
    files: &[SourceFile],
    auxiliary: &mut Vec<SourceFile>,
) -> Result<PlanOutcome, Diagnostic> {
    let project_paths: Vec<&str> = files
        .iter()
        .filter(|file| !file.embedded)
        .map(|file| file.path.as_ref())
        .collect();
    let file_paths: Vec<&str> = files.iter().map(|file| file.path.as_ref()).collect();
    let mut registry = Catalog {
        project,
        files,
        auxiliary,
        first_auxiliary: files.len(),
    };
    axiom_sync::plan(
        book,
        run,
        &project.root,
        today,
        wanted,
        &project_paths,
        &file_paths,
        &mut registry,
    )
}

struct Catalog<'a> {
    project: &'a Project,
    files: &'a [SourceFile],
    auxiliary: &'a mut Vec<SourceFile>,
    first_auxiliary: usize,
}

impl SourceRegistry for Catalog<'_> {
    fn read(&mut self, path: &str) -> Result<Option<FileId>, Diagnostic> {
        // A declared reader may name an Axiom source already loaded by the
        // project. Reuse that exact text and identity instead of reading or
        // registering a duplicate.
        if let Some(source) = self
            .files
            .iter()
            .find(|source| !source.embedded && source.path == path)
        {
            return Ok(Some(source.id));
        }
        let text = self.project.read_local(path)?;
        let file = Sources::append_auxiliary_to(
            self.auxiliary,
            self.first_auxiliary,
            path.to_owned(),
            text,
        )?;
        Ok(Some(file))
    }

    fn text(&self, file: FileId) -> Option<&str> {
        self.files
            .iter()
            .chain(self.auxiliary.iter())
            .find(|source| source.id == file)
            .map(|source| source.text.as_ref())
    }

    fn generated(&mut self, path: &str, text: String) -> Result<FileId, Diagnostic> {
        Sources::append_auxiliary_to(
            self.auxiliary,
            self.first_auxiliary,
            path.to_owned(),
            text,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    #[test]
    fn local_reader_and_generated_text_receive_real_append_only_ids() {
        let dir = TempDir::new("sync-catalog");
        dir.write("axiom.ax", "base USD\n");
        dir.write("statement.csv", "date,amount,memo\n");
        let project = Project::find(dir.path()).unwrap();
        let mut sources = project.load().unwrap();
        let first_auxiliary = sources.files.len();
        let (files, auxiliary) = (&sources.files, &mut sources.auxiliary);
        let mut catalog = Catalog {
            project: &project,
            files,
            auxiliary,
            first_auxiliary,
        };

        let input = catalog.read("statement.csv").unwrap().unwrap();
        assert_eq!(catalog.text(input), Some("date,amount,memo\n"));
        let generated = catalog
            .generated("out.ax", "2026-01-01 a -> b 1 USD\n".to_owned())
            .unwrap();
        assert_eq!(generated.0, input.0 + 1);
        assert_eq!(catalog.text(generated), Some("2026-01-01 a -> b 1 USD\n"));
    }
}

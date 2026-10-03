//! The texts a project is read from, by file number: what a diagnostic's `Loc` points into.
//!
//! [`Sources`] is a table of `&SourceFile` indexed by [`FileId`], over the [`Texts`] that own the files. A table
//! of references and not a `Vec<SourceFile>` because a table is *cloned*: a hypothetical edit swaps one entry, a sync
//! adds the data files it read, and neither copies a text or disturbs the book that is borrowing the others. The
//! project's files come first, in path order, then the systems that ship with Axiom, then (appended later, never
//! parsed as Axiom) the data files a reader names; so a `FileId` is a position, and stays one.

use std::borrow::Cow;
use std::sync::OnceLock;

use axiom_core::{Diagnostic, FileId, Loc, par};
use axiom_model::Source;
use axiom_report::{SourcePosition, SourceProvider};

use crate::Texts;

/// A project's own systems live here, and win over embedded ones of the same path.
const SYSTEMS_DIR: &str = "systems/";

/// How many files a `FileId` can number.
const FILE_LIMIT: usize = u16::MAX as usize + 1;

/// One source text and where it came from.
pub struct SourceFile {
    /// Assigned in order: the project's files by path, then embedded systems.
    pub id: FileId,
    /// Relative to the project root; the system path for embedded systems.
    pub path: Cow<'static, str>,
    pub text: Cow<'static, str>,
    /// Shipped with Axiom rather than found in the project.
    pub embedded: bool,
    /// The byte at which each line starts, found when something first points into the file.
    starts: OnceLock<Vec<usize>>,
}

impl SourceFile {
    pub(crate) fn new(id: FileId, path: Cow<'static, str>, text: Cow<'static, str>, embedded: bool) -> SourceFile {
        SourceFile { id, path, text, embedded, starts: OnceLock::new() }
    }

    fn starts(&self) -> &[usize] {
        self.starts.get_or_init(|| {
            let newlines = memchr::memchr_iter(b'\n', self.text.as_bytes()).map(|at| at + 1);
            std::iter::once(0).chain(newlines).collect()
        })
    }

    /// How many lines the text has.
    pub fn lines(&self) -> usize {
        self.starts().len()
    }

    /// The line (counting from 0) that holds byte `offset`. An offset past the end belongs to the last line.
    pub fn line_of(&self, offset: usize) -> usize {
        self.starts().partition_point(|&start| start <= offset) - 1
    }

    /// Where `line` starts. A line past the end starts at the end.
    pub fn line_start(&self, line: usize) -> usize {
        self.starts().get(line).copied().unwrap_or(self.text.len())
    }

    /// The text of `line`, without its line ending.
    pub fn line(&self, line: usize) -> &str {
        self.text[self.line_start(line)..self.line_start(line + 1)].trim_end_matches(['\n', '\r'])
    }

    fn position(&self, loc: Loc) -> Option<SourcePosition<'_>> {
        let (offset, end) = (loc.start as usize, loc.end as usize);
        if offset > end
            || end > self.text.len()
            || !self.text.is_char_boundary(offset)
            || !self.text.is_char_boundary(end)
        {
            return None;
        }
        let line = self.line_of(offset);
        let start = self.line_start(line);
        let column =
            self.text[start..].char_indices().take_while(|(relative, _)| start + *relative < offset).count() + 1;
        Some(SourcePosition { path: &self.path, line: line + 1, column })
    }
}

/// Every source text of a run. The syntax tree, the book and every diagnostic borrow from the [`Texts`] these are
/// kept in, so those outlive them all.
#[derive(Clone)]
pub struct Sources<'t> {
    texts: &'t Texts,
    /// Axiom inputs: the project's files, then the embedded systems.
    files: Vec<&'t SourceFile>,
    /// Data read by sync and `check`, with numbers after all of the Axiom inputs.
    auxiliary: Vec<&'t SourceFile>,
}

impl<'t> Sources<'t> {
    /// No file yet, to be kept in `texts`.
    pub fn empty(texts: &'t Texts) -> Sources<'t> {
        Sources { texts, files: Vec::new(), auxiliary: Vec::new() }
    }

    /// The project's files first, then each embedded system the project does not override. Embedded texts are
    /// borrowed, never copied.
    pub fn assemble(
        texts: &'t Texts,
        project: Vec<(String, String)>,
        systems: &'static [(&'static str, &'static str)],
    ) -> Result<Sources<'t>, Diagnostic> {
        let inherited: Vec<_> = systems.iter().filter(|(path, _)| !overridden(&project, path)).collect();
        if project.len() + inherited.len() > FILE_LIMIT {
            return Err(Diagnostic::error(
                "too-many-files",
                format!("too many source files: at most {FILE_LIMIT} are supported"),
            ));
        }
        let own = project.into_iter().map(|(path, text)| (Cow::Owned(path), Cow::Owned(text), false));
        let embedded = inherited.into_iter().map(|&(path, text)| (Cow::Borrowed(path), Cow::Borrowed(text), true));
        let files = own
            .chain(embedded)
            .enumerate()
            .map(|(index, (path, text, embedded))| {
                texts.keep(SourceFile::new(FileId(index as u16), path, text, embedded))
            })
            .collect();
        Ok(Sources { texts, files, auxiliary: Vec::new() })
    }

    /// Texts that are not on disk, as project files in the order given, and then `systems` as the embedded ones.
    pub fn in_memory(
        texts: &'t Texts,
        files: &[(&str, &str)],
        systems: &'static [(&'static str, &'static str)],
    ) -> Sources<'t> {
        let owned = files.iter().map(|&(path, text)| (path.to_string(), text.to_string())).collect();
        Sources::assemble(texts, owned, systems).expect("a handful of files")
    }

    /// Where more texts are kept: `append_auxiliary` and an applied edit add to it.
    pub fn texts(&self) -> &'t Texts {
        self.texts
    }

    /// The source with this id, if there is one.
    pub fn get(&self, id: FileId) -> Option<&'t SourceFile> {
        let index = usize::from(id.0);
        self.files.get(index).or_else(|| self.auxiliary.get(index.checked_sub(self.files.len())?)).copied()
    }

    /// The Axiom sources, in `FileId` order: the project's, then the embedded systems.
    pub fn files(&self) -> impl Iterator<Item = &'t SourceFile> + '_ {
        self.files.iter().copied()
    }

    /// The data files appended so far, in `FileId` order.
    pub fn auxiliary(&self) -> impl Iterator<Item = &'t SourceFile> + '_ {
        self.auxiliary.iter().copied()
    }

    /// Adds a data file so diagnostics from a declared reader can point into it. Its id follows every Axiom
    /// source, and it is never parsed as Axiom or selected by commands that write project sources.
    pub fn append_auxiliary(&mut self, path: String, text: String) -> Result<FileId, Diagnostic> {
        let Ok(index) = u16::try_from(self.files.len() + self.auxiliary.len()) else {
            return Err(Diagnostic::error("too-many-files", "too many source files: at most 65,536 are supported"));
        };
        let file = SourceFile::new(FileId(index), Cow::Owned(path), Cow::Owned(text), false);
        self.auxiliary.push(self.texts.keep(file));
        Ok(FileId(index))
    }

    /// The relative paths of project-owned `.ax` files, in parse order. Embedded standard systems are
    /// deliberately omitted for commands such as `fmt` and `sync` that operate on files in the project directory.
    pub fn project_paths(&self) -> impl Iterator<Item = &'t str> + '_ {
        self.files.iter().filter(|file| !file.embedded).map(|&file| &*file.path)
    }

    /// Every path in `FileId` order, including embedded systems and appended reader data. Model locations store
    /// only the numeric file id, so clients use this sequence to resolve a declaration or diagnostic to its path.
    pub fn all_paths(&self) -> impl Iterator<Item = &'t str> + '_ {
        self.files.iter().chain(&self.auxiliary).map(|&file| &*file.path)
    }

    /// The source at `path`, as `axiom why` or a diagnostic shows it.
    pub fn find(&self, path: &str) -> Option<&'t SourceFile> {
        self.files.iter().chain(&self.auxiliary).copied().find(|file| file.path == path)
    }

    /// `journal/2026/01.ax:14`: what `axiom why` accepts back.
    pub fn describe(&self, loc: Loc) -> Option<String> {
        let file = self.get(loc.file)?;
        Some(format!("{}:{}", file.path, file.line_of(loc.start as usize) + 1))
    }

    /// The bytes of line `number` (counted from 1) of the file at `path`.
    pub fn locate(&self, path: &str, number: usize) -> Option<Loc> {
        let file = self.find(path)?;
        let line = number.checked_sub(1).filter(|&line| line < file.lines())?;
        let start = file.line_start(line);
        Some(Loc::new(file.id, start as u32, (start + file.line(line).len()) as u32))
    }

    /// Parses every Axiom file, in parallel, into what the model builds from. The trees borrow the texts, not
    /// `self`, so they can be built into a book that outlives this table.
    pub fn parse(&self) -> (Vec<Source<'t>>, Vec<Diagnostic>) {
        let parsed = par::map_each(&self.files, |&file| {
            axiom_syntax::parse(file.id, &file.text, axiom_syntax::Folder::of(&file.path))
        });
        let mut diagnostics = Vec::new();
        let sources = self
            .files
            .iter()
            .zip(parsed)
            .map(|(&file, (ast, found))| {
                diagnostics.extend(found);
                Source { path: &file.path, file: ast, embedded: file.embedded }
            })
            .collect();
        (sources, diagnostics)
    }
}

impl SourceProvider for Sources<'_> {
    fn locate(&self, path: &str, line: usize) -> Option<Loc> {
        Sources::locate(self, path, line)
    }

    fn describe(&self, loc: Loc) -> Option<SourcePosition<'_>> {
        self.get(loc.file)?.position(loc)
    }
}

fn overridden(project: &[(String, String)], system: &str) -> bool {
    project.iter().any(|(path, _)| path.strip_prefix(SYSTEMS_DIR) == Some(system))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts_of<'a>(sources: &'a Sources<'_>) -> Vec<(&'a str, bool)> {
        sources.files().map(|file| (&*file.path, file.embedded)).collect()
    }

    #[test]
    fn project_systems_override_embedded_ones_by_path() {
        static EMBEDDED: [(&str, &str); 2] = [("us.ax", "system us"), ("us/401k.ax", "system us/401k")];
        let project = vec![
            ("systems/us.ax".to_string(), "system us // mine".to_string()),
            ("journal.ax".to_string(), String::new()),
        ];
        let texts = Texts::default();
        let sources = Sources::assemble(&texts, project, &EMBEDDED).unwrap();
        assert_eq!(texts_of(&sources), [("systems/us.ax", false), ("journal.ax", false), ("us/401k.ax", true)]);
        assert_eq!(sources.all_paths().collect::<Vec<_>>(), ["systems/us.ax", "journal.ax", "us/401k.ax"]);
        assert_eq!(sources.project_paths().collect::<Vec<_>>(), ["systems/us.ax", "journal.ax"]);
        assert_eq!(sources.get(FileId(2)).map(|file| file.id), Some(FileId(2)));
        assert!(sources.get(FileId(3)).is_none());
    }

    #[test]
    fn lines_and_offsets() {
        let texts = Texts::default();
        let sources = Sources::in_memory(&texts, &[("a.ax", "ab\ncd\r\n\nlast")], &[]);
        let file = sources.get(FileId(0)).unwrap();
        assert_eq!([0, 2, 3, 5, 6, 7, 8, 100].map(|at| file.line_of(at)), [0, 0, 1, 1, 1, 2, 3, 3]);
        assert_eq!((file.line(1), file.line(2), file.line(3), file.line(9)), ("cd", "", "last", ""));
        assert_eq!(file.lines(), 4);
        // A final newline starts an empty line.
        let sources = Sources::in_memory(&texts, &[("b.ax", "a\n")], &[]);
        assert_eq!(sources.get(FileId(0)).unwrap().lines(), 2);
    }

    #[test]
    fn a_line_is_described_and_found_again() {
        let texts = Texts::default();
        let sources = Sources::in_memory(&texts, &[("journal/2026/01.ax", "one\ntwo\n")], &[]);
        let two = sources.locate("journal/2026/01.ax", 2).unwrap();
        assert_eq!((two.start, two.end), (4, 7));
        assert_eq!(sources.describe(two).as_deref(), Some("journal/2026/01.ax:2"));
        assert!(sources.locate("journal/2026/01.ax", 4).is_none() && sources.locate("nowhere.ax", 1).is_none());
    }

    #[test]
    fn borrowed_positions_count_unicode_and_reject_invalid_byte_ranges() {
        let texts = Texts::default();
        let sources = Sources::in_memory(&texts, &[("journal/λ.ax", "a\tλ\nnext")], &[]);
        let path = "journal/λ.ax";
        let at_letter = sources.locate(path, 1).unwrap();
        let position = SourceProvider::describe(&sources, Loc::new(at_letter.file, 2, 4)).unwrap();
        assert_eq!((position.path, position.line, position.column), (path, 1, 3));
        assert!(SourceProvider::describe(&sources, Loc::new(at_letter.file, 3, 4)).is_none());
        assert!(SourceProvider::describe(&sources, Loc::new(at_letter.file, 0, u32::MAX)).is_none());
        assert!(SourceProvider::describe(&sources, Loc::new(at_letter.file, 4, 2)).is_none());
    }

    #[test]
    fn auxiliary_data_keeps_file_ids_without_becoming_a_project_source() {
        let texts = Texts::default();
        let mut sources = Sources::in_memory(&texts, &[("axiom.ax", "")], &[]);
        let (parsed, diagnostics) = sources.parse();
        let csv = sources.append_auxiliary("imports/bank.csv".to_string(), "date,amount\nbad".to_string()).unwrap();

        assert_eq!(parsed.len(), 1);
        assert!(diagnostics.is_empty());
        assert_eq!(csv, FileId(1));
        assert_eq!(sources.project_paths().collect::<Vec<_>>(), ["axiom.ax"]);
        assert_eq!(sources.all_paths().collect::<Vec<_>>(), ["axiom.ax", "imports/bank.csv"]);

        let location = sources.locate("imports/bank.csv", 2).unwrap();
        assert_eq!(location.file, csv);
        let position = SourceProvider::describe(&sources, location).unwrap();
        assert_eq!((position.path, position.line, position.column), ("imports/bank.csv", 2, 1));
        // The book that borrows the parsed files is not disturbed by the data file appended while it lives.
        drop(parsed);

        // The ordinary parser always excludes reader data too.
        let (parsed, diagnostics) = sources.parse();
        assert_eq!(parsed.len(), 1);
        assert!(diagnostics.is_empty());
    }
}

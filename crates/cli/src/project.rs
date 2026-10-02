//! Finding a project on disk and reading its sources.
//!
//! A project is a folder with an `axiom.ax` in it; every `.ax` file below is
//! part of it. A lone `.ax` file with no project above it is a one-file project.
//! The standard systems are embedded in the binary and join every project, unless
//! the project's own `systems/` folder has a file at the same path.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use axiom_core::{Diagnostic, FileId, Loc, par};
use axiom_model::Source;
use axiom_report::{SourcePosition, SourceProvider};

/// The file that marks a project's root.
const MARKER: &str = "axiom.ax";
const EXTENSION: &str = "ax";
/// A project's own systems live here, and win over embedded ones of the same path.
const SYSTEMS_DIR: &str = "systems/";

/// A project found on disk.
pub struct Project {
    /// Where relative paths start, and where `sync` commands run.
    pub root: PathBuf,
    /// The one file of a one-file project; everything under `root` otherwise.
    only: Option<PathBuf>,
}

impl Project {
    /// The project around `start`: the nearest folder at or above it with an
    /// `axiom.ax`. A `.ax` file with no such folder above it is a project alone.
    pub fn find(start: &Path) -> Result<Project, Diagnostic> {
        let start = fs::canonicalize(start)
            .map_err(|error| failure("unreadable", format!("cannot open {}: {error}", start.display())))?;
        let alone = start.is_file();
        if alone && start.extension() != Some(OsStr::new(EXTENSION)) {
            return Err(failure("no-project", format!("{} is not a folder or a .{EXTENSION} file", start.display())));
        }
        let folder = if alone { start.parent().unwrap_or(&start) } else { &start };
        match marker_above(folder) {
            Some(root) => Ok(Project { root, only: None }),
            None if alone => Ok(Project { root: folder.to_path_buf(), only: Some(start.clone()) }),
            None => Err(failure("no-project", format!("no {MARKER} in {} or any folder above it", start.display()))
                .help(format!("create an `{MARKER}` (it may be empty) in the folder that holds your ledger"))
                .help("or point at one file on its own: `axiom -C FILE.ax check`")),
        }
    }

    /// Reads every source of the project, and the systems that come with it.
    /// The files are read, and checked to be UTF-8, by every core at once.
    pub fn load(&self) -> Result<Sources, Diagnostic> {
        let relative = match &self.only {
            Some(file) => file.file_name().map(PathBuf::from).into_iter().collect(),
            None => self.find_sources()?,
        };
        let texts = par::map_each(&relative, |path| self.read(path));
        let files = relative.iter().map(|path| display(path)).zip(texts);
        Sources::assemble(
            files.map(|(path, text)| Ok((path, text?))).collect::<Result<_, _>>()?,
            axiom_systems::SYSTEMS,
        )
    }

    /// The text of the file at `relative`.
    fn read(&self, relative: &Path) -> Result<String, Diagnostic> {
        let shown = display(relative);
        let bytes = fs::read(self.root.join(relative))
            .map_err(|error| failure("unreadable", format!("cannot read {shown}: {error}")))?;
        String::from_utf8(bytes).map_err(|error| {
            let before = &error.as_bytes()[..error.utf8_error().valid_up_to()];
            let line = 1 + memchr::memchr_iter(b'\n', before).count();
            failure("not-utf8", format!("cannot read {shown}: line {line} is not valid UTF-8"))
                .help("save the file as UTF-8")
        })
    }

    /// Reads a local sync input only after resolving its final target beneath
    /// the canonical project root. The sync glob check is repeated here so a
    /// path changed between expansion and reading cannot escape through a
    /// symlink.
    pub fn read_local(&self, relative: &str) -> Result<String, Diagnostic> {
        let path = self.root.join(relative);
        let canonical = fs::canonicalize(&path)
            .map_err(|error| failure("sync-read-path", format!("cannot resolve `{relative}`: {error}")))?;
        if !canonical.starts_with(&self.root) {
            return Err(failure("sync-read-path", format!("`{relative}` leaves the project through a symlink")));
        }
        let bytes = fs::read(&canonical)
            .map_err(|error| failure("sync-read-path", format!("cannot read `{relative}`: {error}")))?;
        String::from_utf8(bytes).map_err(|error| {
            let before = &error.as_bytes()[..error.utf8_error().valid_up_to()];
            let line = 1 + memchr::memchr_iter(b'\n', before).count();
            failure("not-utf8", format!("cannot read `{relative}`: line {line} is not valid UTF-8"))
        })
    }

    fn find_sources(&self) -> Result<Vec<PathBuf>, Diagnostic> {
        let mut found = Vec::new();
        collect(&self.root, Path::new(""), &mut found)
            .map_err(|error| failure("unreadable", format!("cannot list {}: {error}", self.root.display())))?;
        // Path order compares folder by folder, so a folder's files stay
        // together and declaration order is the same on every machine.
        found.sort();
        Ok(found)
    }
}

fn marker_above(folder: &Path) -> Option<PathBuf> {
    folder.ancestors().find(|candidate| candidate.join(MARKER).is_file()).map(Path::to_path_buf)
}

/// Adds every `.ax` file under `root/relative` to `found`, as paths relative to
/// `root`. Hidden entries and `target/` are skipped. A link to a file counts as
/// the file (a dangling one is nothing); a link to a folder is not followed,
/// since it could lead around in circles.
fn collect(root: &Path, relative: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') || name == "target" {
            continue;
        }
        let path = relative.join(&name);
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(root, &path, found)?;
        } else if path.extension() == Some(OsStr::new(EXTENSION))
            && fs::metadata(entry.path()).is_ok_and(|m| m.is_file())
        {
            found.push(path);
        }
    }
    Ok(())
}

/// `journal/2026/01.ax`, with `/` on every platform.
fn display(path: &Path) -> String {
    let parts: Vec<_> = path.components().map(|part| part.as_os_str().to_string_lossy()).collect();
    parts.join("/")
}

fn failure(code: &'static str, message: String) -> Diagnostic {
    Diagnostic::error(code, message)
}

/// One source text and where it came from.
pub struct SourceFile {
    /// Assigned in order: the project's files by path, then embedded systems.
    pub id: FileId,
    /// Relative to the project root; the system path for embedded systems.
    pub path: Cow<'static, str>,
    pub text: Cow<'static, str>,
    /// Shipped with Axiom rather than found in the project.
    pub embedded: bool,
    /// The byte at which each line starts, found when something first points
    /// into the file.
    starts: OnceLock<Vec<usize>>,
}

impl SourceFile {
    fn new(id: FileId, path: Cow<'static, str>, text: Cow<'static, str>, embedded: bool) -> SourceFile {
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

    /// The line (counting from 0) that holds byte `offset`. An offset past the
    /// end belongs to the last line.
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

/// Every source text of a run. The syntax tree, the book and every diagnostic
/// borrow from here, so it outlives them all.
#[derive(Default)]
pub struct Sources {
    /// Axiom inputs borrowed by the syntax tree and model.
    pub(super) files: Vec<SourceFile>,
    /// Data read by sync/check, with IDs after all syntax sources.
    pub(super) auxiliary: Vec<SourceFile>,
}

impl Sources {
    /// One text that is not on disk: what a `sync` command printed.
    pub fn single(path: String, text: String) -> Sources {
        let file = SourceFile::new(FileId(0), Cow::Owned(path), Cow::Owned(text), false);
        Sources { files: vec![file], auxiliary: Vec::new() }
    }

    /// The project's files first, then each embedded system the project does
    /// not override. Embedded texts are borrowed, never copied.
    fn assemble(
        project: Vec<(String, String)>,
        systems: &'static [(&'static str, &'static str)],
    ) -> Result<Sources, Diagnostic> {
        let inherited: Vec<_> = systems.iter().filter(|(path, _)| !overridden(&project, path)).collect();
        let limit = usize::from(u16::MAX) + 1;
        if project.len() + inherited.len() > limit {
            return Err(failure("too-many-files", format!("too many source files: at most {limit} are supported")));
        }
        let own = project.into_iter().map(|(path, text)| (Cow::Owned(path), Cow::Owned(text), false));
        let embedded = inherited.into_iter().map(|&(path, text)| (Cow::Borrowed(path), Cow::Borrowed(text), true));
        let files = own
            .chain(embedded)
            .enumerate()
            .map(|(index, (path, text, embedded))| SourceFile::new(FileId(index as u16), path, text, embedded))
            .collect();
        Ok(Sources { files, auxiliary: Vec::new() })
    }

    /// Texts that are not on disk, as project files in the order given, and
    /// then `systems` as the embedded ones.
    #[cfg(test)]
    pub fn in_memory(files: &[(&str, &str)], systems: &'static [(&'static str, &'static str)]) -> Sources {
        let texts = files.iter().map(|&(path, text)| (path.to_string(), text.to_string())).collect();
        Sources::assemble(texts, systems).expect("a handful of files")
    }

    /// The source with this id, if there is one.
    pub fn get(&self, id: FileId) -> Option<&SourceFile> {
        let index = usize::from(id.0);
        self.files.get(index).or_else(|| self.auxiliary.get(index.checked_sub(self.files.len())?))
    }

    /// Adds a data file so diagnostics from a declared reader can point into
    /// it. Its id follows every parsed Axiom source, and it is never parsed as
    /// Axiom or selected by commands that write project sources.
    pub fn append_auxiliary(&mut self, path: String, text: String) -> Result<FileId, Diagnostic> {
        Self::append_auxiliary_to(&mut self.auxiliary, self.files.len(), path, text)
    }

    /// Appends reader text separately from syntax files, so callers can add
    /// it while a `Book` borrows those syntax files.
    pub(super) fn append_auxiliary_to(
        auxiliary: &mut Vec<SourceFile>,
        first_id: usize,
        path: String,
        text: String,
    ) -> Result<FileId, Diagnostic> {
        let Ok(index) = u16::try_from(first_id + auxiliary.len()) else {
            return Err(failure("too-many-files", "too many source files: at most 65,536 are supported".to_string()));
        };
        let mut file = SourceFile::new(FileId(index), Cow::Owned(path), Cow::Owned(text), false);
        auxiliary.push(file);
        Ok(FileId(index))
    }

    /// The relative paths of project-owned `.ax` files, in parse order.
    /// Embedded standard systems are deliberately omitted for commands such
    /// as `fmt` and `sync` that operate on files in the project directory.
    pub fn project_paths(&self) -> impl Iterator<Item = &str> {
        self.files.iter().filter(|file| !file.embedded).map(|file| &*file.path)
    }

    /// Every path in `FileId` order, including embedded systems and appended
    /// reader data. Model locations store only the numeric file id, so clients
    /// use this sequence to resolve a declaration or diagnostic to its path.
    pub fn all_paths(&self) -> impl Iterator<Item = &str> {
        self.files.iter().chain(&self.auxiliary).map(|file| &*file.path)
    }

    /// The source at `path`, as `axiom why` or a diagnostic shows it.
    pub fn find(&self, path: &str) -> Option<&SourceFile> {
        self.files.iter().chain(&self.auxiliary).find(|file| file.path == path)
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

    fn position(&self, loc: Loc) -> Option<SourcePosition<'_>> {
        let file = self.get(loc.file)?;
        file.position(loc)
    }

    /// Parses every file, in parallel, into what the model builds from.
    pub fn parse(&self) -> (Vec<Source<'_>>, Vec<Diagnostic>) {
        Self::parse_files(&self.files)
    }

    /// Parses only syntax files, so returned model borrows do not block writes
    /// to the disjoint reader-data collection.
    pub(super) fn parse_files(files: &[SourceFile]) -> (Vec<Source<'_>>, Vec<Diagnostic>) {
        let parsed =
            par::map_each(files, |file| axiom_syntax::parse(file.id, &file.text, axiom_syntax::Folder::of(&file.path)));
        let mut diagnostics = Vec::new();
        let sources = files
            .iter()
            .zip(parsed)
            .map(|(file, (ast, found))| {
                diagnostics.extend(found);
                Source { path: &file.path, file: ast, embedded: file.embedded }
            })
            .collect();
        (sources, diagnostics)
    }
}

impl SourceProvider for Sources {
    fn locate(&self, path: &str, line: usize) -> Option<Loc> {
        Sources::locate(self, path, line)
    }

    fn describe(&self, loc: Loc) -> Option<SourcePosition<'_>> {
        Sources::position(self, loc)
    }
}

fn overridden(project: &[(String, String)], system: &str) -> bool {
    project.iter().any(|(path, _)| path.strip_prefix(SYSTEMS_DIR) == Some(system))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    fn texts(sources: &Sources) -> Vec<(&str, bool)> {
        sources.files.iter().map(|file| (&*file.path, file.embedded)).collect()
    }

    /// The project's own files, without the embedded systems every project gets.
    fn own(sources: &Sources) -> Vec<&str> {
        sources.files.iter().filter(|file| !file.embedded).map(|file| &*file.path).collect()
    }

    #[test]
    fn finds_the_root_from_below_and_lists_files_in_path_order() {
        let dir = TempDir::new("root");
        dir.write("axiom.ax", "base USD\n");
        dir.write("journal/2026/02.ax", "");
        dir.write("journal/2026/01.ax", "");
        dir.write("journal/2026.ax", "");
        dir.write("target/junk.ax", "");
        dir.write(".hidden/secret.ax", "");
        dir.write("notes.txt", "");

        let project = Project::find(&dir.path().join("journal/2026")).unwrap();
        assert_eq!(project.root, fs::canonicalize(dir.path()).unwrap());
        let sources = project.load().unwrap();
        assert_eq!(own(&sources), ["axiom.ax", "journal/2026/01.ax", "journal/2026/02.ax", "journal/2026.ax"]);
    }

    #[test]
    fn links_to_files_count_and_links_to_folders_are_not_followed() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new("links");
        dir.write("axiom.ax", "");
        dir.write("shared/prices.ax", "");
        dir.write("shared/loop/inner.ax", "");
        symlink(dir.path().join("shared/prices.ax"), dir.path().join("linked.ax")).unwrap();
        symlink(dir.path().join("missing.ax"), dir.path().join("dangling.ax")).unwrap();
        symlink(dir.path(), dir.path().join("shared/loop/back")).unwrap();

        let sources = Project::find(dir.path()).unwrap().load().unwrap();
        assert_eq!(own(&sources), ["axiom.ax", "linked.ax", "shared/loop/inner.ax", "shared/prices.ax"]);
    }

    #[test]
    fn a_lone_file_is_a_project_but_a_bare_folder_is_not() {
        let dir = TempDir::new("lone");
        dir.write("first-steps.ax", "2026-01-01 a -> b 1 USD\n");

        let project = Project::find(&dir.path().join("first-steps.ax")).unwrap();
        assert_eq!(own(&project.load().unwrap()), ["first-steps.ax"]);

        let error = Project::find(dir.path()).err().unwrap();
        assert!(error.message.contains("no axiom.ax"), "{}", error.message);
        let error = Project::find(&dir.path().join("missing")).err().unwrap();
        assert!(error.message.starts_with("cannot open"), "{}", error.message);
    }

    #[test]
    fn project_systems_override_embedded_ones_by_path() {
        static EMBEDDED: [(&str, &str); 2] = [("us.ax", "system us"), ("us/401k.ax", "system us/401k")];
        let project = vec![
            ("systems/us.ax".to_string(), "system us // mine".to_string()),
            ("journal.ax".to_string(), String::new()),
        ];
        let sources = Sources::assemble(project, &EMBEDDED).unwrap();
        assert_eq!(texts(&sources), [("systems/us.ax", false), ("journal.ax", false), ("us/401k.ax", true)]);
        assert_eq!(sources.all_paths().collect::<Vec<_>>(), ["systems/us.ax", "journal.ax", "us/401k.ax"]);
        assert_eq!(sources.project_paths().collect::<Vec<_>>(), ["systems/us.ax", "journal.ax"]);
        assert_eq!(sources.get(FileId(2)).map(|file| file.id), Some(FileId(2)));
        assert!(sources.get(FileId(3)).is_none());
    }

    #[test]
    fn lines_and_offsets() {
        let sources = Sources::in_memory(&[("a.ax", "ab\ncd\r\n\nlast")], &[]);
        let file = sources.get(FileId(0)).unwrap();
        assert_eq!([0, 2, 3, 5, 6, 7, 8, 100].map(|at| file.line_of(at)), [0, 0, 1, 1, 1, 2, 3, 3]);
        assert_eq!((file.line(1), file.line(2), file.line(3), file.line(9)), ("cd", "", "last", ""));
        assert_eq!(file.lines(), 4);
        // A final newline starts an empty line.
        let sources = Sources::in_memory(&[("b.ax", "a\n")], &[]);
        assert_eq!(sources.get(FileId(0)).unwrap().lines(), 2);
    }

    #[test]
    fn a_line_is_described_and_found_again() {
        let sources = Sources::in_memory(&[("journal/2026/01.ax", "one\ntwo\n")], &[]);
        let two = sources.locate("journal/2026/01.ax", 2).unwrap();
        assert_eq!((two.start, two.end), (4, 7));
        assert_eq!(sources.describe(two).as_deref(), Some("journal/2026/01.ax:2"));
        assert!(sources.locate("journal/2026/01.ax", 4).is_none() && sources.locate("nowhere.ax", 1).is_none());
    }

    #[test]
    fn borrowed_positions_count_unicode_and_reject_invalid_byte_ranges() {
        let sources = Sources::in_memory(&[("journal/λ.ax", "a\tλ\nnext")], &[]);
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
        let mut sources = Sources::in_memory(&[("axiom.ax", "")], &[]);
        let (parsed, diagnostics) = Sources::parse_files(&sources.files);
        let csv = Sources::append_auxiliary_to(
            &mut sources.auxiliary,
            sources.files.len(),
            "imports/bank.csv".to_string(),
            "date,amount\nbad".to_string(),
        )
        .unwrap();

        assert_eq!(parsed.len(), 1);
        assert!(diagnostics.is_empty());
        assert_eq!(csv, FileId(1));
        assert_eq!(sources.project_paths().collect::<Vec<_>>(), ["axiom.ax"]);
        assert_eq!(sources.all_paths().collect::<Vec<_>>(), ["axiom.ax", "imports/bank.csv"]);

        let location = sources.locate("imports/bank.csv", 2).unwrap();
        assert_eq!(location.file, csv);
        let position = SourceProvider::describe(&sources, location).unwrap();
        assert_eq!((position.path, position.line, position.column), ("imports/bank.csv", 2, 1));
        drop(parsed);

        // The ordinary parser always excludes reader data too.
        let (parsed, diagnostics) = sources.parse();
        assert_eq!(parsed.len(), 1);
        assert!(diagnostics.is_empty());
    }
}

//! Finding a project on disk and reading its sources.
//!
//! A project is a folder with an `axiom.ax` in it; every `.ax` file below is
//! part of it. A lone `.ax` file with no project above it is a one-file project.
//! The standard systems are embedded in the binary and join every project, unless
//! the project's own `systems/` folder has a file at the same path.
//!
//! This is the one part of loading that is file IO, so it is the command line's: a
//! session is given its [`Sources`], and a client with buffers instead of folders
//! (an editor, a GUI) makes them its own way.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axiom_core::{Diagnostic, par};
use axiom_session::{Sources, Texts};

/// The file that marks a project's root.
const MARKER: &str = "axiom.ax";
const EXTENSION: &str = "ax";

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

    /// Reads every source of the project, and the systems that come with it, into `texts`.
    /// The files are read, and checked to be UTF-8, by every core at once.
    pub fn load<'t>(&self, texts: &'t Texts) -> Result<Sources<'t>, Diagnostic> {
        let relative = match &self.only {
            Some(file) => file.file_name().map(PathBuf::from).into_iter().collect(),
            None => self.find_sources()?,
        };
        let contents = par::map_each(&relative, |path| self.read(path));
        let files = relative.iter().map(|path| display(path)).zip(contents);
        Sources::assemble(
            texts,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TempDir;

    /// The project's own files, without the embedded systems every project gets.
    fn own<'a>(sources: &'a Sources<'_>) -> Vec<&'a str> {
        sources.project_paths().collect()
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
        let texts = Texts::default();
        let sources = project.load(&texts).unwrap();
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

        let texts = Texts::default();
        let sources = Project::find(dir.path()).unwrap().load(&texts).unwrap();
        assert_eq!(own(&sources), ["axiom.ax", "linked.ax", "shared/loop/inner.ax", "shared/prices.ax"]);
    }

    #[test]
    fn a_lone_file_is_a_project_but_a_bare_folder_is_not() {
        let dir = TempDir::new("lone");
        dir.write("first-steps.ax", "2026-01-01 a -> b 1 USD\n");

        let project = Project::find(&dir.path().join("first-steps.ax")).unwrap();
        let texts = Texts::default();
        assert_eq!(own(&project.load(&texts).unwrap()), ["first-steps.ax"]);

        let error = Project::find(dir.path()).err().unwrap();
        assert!(error.message.contains("no axiom.ax"), "{}", error.message);
        let error = Project::find(&dir.path().join("missing")).err().unwrap();
        assert!(error.message.starts_with("cannot open"), "{}", error.message);
    }
}

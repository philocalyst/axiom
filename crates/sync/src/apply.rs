//! Writing a plan: the one place sync touches the project's files.
//!
//! Planning only produces [`Change`]s, the text each file would hold, so that whoever asked for the plan (the
//! command line, a server, an editor) can show it before anything is written. Applying one is the file system's
//! business and stays here, in one place that every caller shares. A change is staged in a sibling file and
//! renamed over its target, so a reader never sees half a file, and every path is resolved through the file
//! system first, so a symlink cannot carry a write out of the project.

use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use axiom_core::Diagnostic;

use crate::Change;

/// How many names a staged file is offered before giving up.
const STAGING_NAMES: usize = 100;

/// Writes every change under the project at `root`, and returns what stopped any of them. A change with a problem
/// is not written; the others still are.
pub fn apply(root: &Path, changes: &[Change]) -> Vec<Diagnostic> {
    let root = match fs::canonicalize(root) {
        Ok(root) => root,
        Err(error) => {
            return vec![Diagnostic::error("sync-project-root", format!("cannot resolve the project root: {error}"))];
        }
    };
    let writes = changes.iter().enumerate().map(|(index, change)| write(&root, change, index));
    writes.filter_map(Result::err).map(|problem| Diagnostic::error(problem.code, problem.message)).collect()
}

/// What stopped a change: a diagnostic's code and message, small enough to travel in a `Result`.
struct Problem {
    code: &'static str,
    message: String,
}

fn write(root: &Path, change: &Change, index: usize) -> Result<(), Problem> {
    let (folder, name) = locate(root, change)?;
    let staged = stage(&folder, name, change, index)?;
    fs::rename(&staged, folder.join(name)).map_err(|error| {
        let _ = fs::remove_file(&staged);
        write_failed(format!("cannot replace `{}`: {error}", change.path))
    })
}

/// The folder a change goes in, made if it is missing and resolved through every symlink to somewhere inside
/// `root`, and the file's name there.
fn locate<'c>(root: &Path, change: &'c Change) -> Result<(PathBuf, &'c OsStr), Problem> {
    let relative = Path::new(&change.path);
    if change.path.is_empty() || !relative.components().all(|part| matches!(part, Component::Normal(_))) {
        return Err(outside(format!("`{}` is not a project-relative path", change.path)));
    }
    let (Some(name), Some(inside)) = (relative.file_name(), relative.parent()) else {
        return Err(outside(format!("`{}` has no project directory", change.path)));
    };
    let folder = root.join(inside);
    // The nearest folder that exists is checked before any is made. Otherwise `link/new-folder/file.ax` would
    // create `new-folder` outside the project before the check on the final folder noticed `link`.
    contained(root, nearest_existing(&folder), change)?;
    fs::create_dir_all(&folder)
        .map_err(|error| write_failed(format!("cannot create the folder for `{}`: {error}", change.path)))?;
    Ok((contained(root, fs::canonicalize(&folder), change)?, name))
}

/// The closest ancestor of `path` that exists, resolved through any symlink on the way.
fn nearest_existing(path: &Path) -> io::Result<PathBuf> {
    let mut ancestor = path;
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => return fs::canonicalize(ancestor),
            Err(error) if error.kind() == io::ErrorKind::NotFound => ancestor = ancestor.parent().ok_or(error)?,
            Err(error) => return Err(error),
        }
    }
}

/// `resolved` if it lies inside the project.
fn contained(root: &Path, resolved: io::Result<PathBuf>, change: &Change) -> Result<PathBuf, Problem> {
    match resolved {
        Ok(path) if path.starts_with(root) => Ok(path),
        Ok(_) => Err(outside(format!("`{}` leaves the project through a symlink", change.path))),
        Err(error) => Err(write_failed(format!("cannot resolve the folder for `{}`: {error}", change.path))),
    }
}

/// Writes the change's text to a new file in `folder`, and returns where. Its name carries the process, the
/// change and the attempt, so that nothing else is likely to hold it.
fn stage(folder: &Path, name: &OsStr, change: &Change, index: usize) -> Result<PathBuf, Problem> {
    let process = std::process::id();
    for attempt in 0..STAGING_NAMES {
        let staged = folder.join(format!(".{}.axiom-sync-{process}-{index}-{attempt}", name.to_string_lossy()));
        match OpenOptions::new().write(true).create_new(true).open(&staged) {
            Ok(mut file) => {
                return match file.write_all(change.after.as_bytes()) {
                    Ok(()) => Ok(staged),
                    Err(error) => {
                        let _ = fs::remove_file(&staged);
                        Err(write_failed(format!("cannot write `{}`: {error}", change.path)))
                    }
                };
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(write_failed(format!("cannot prepare `{}`: {error}", change.path))),
        }
    }
    Err(write_failed(format!("cannot prepare `{}`: every staging name is taken", change.path)))
}

fn outside(message: String) -> Problem {
    Problem { code: "sync-path-outside-project", message }
}

fn write_failed(message: String) -> Problem {
    Problem { code: "sync-write", message }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// A fresh project folder, removed when the test is done with it.
    struct Project(PathBuf);

    impl Project {
        fn new() -> Project {
            let root = std::env::temp_dir().join(format!(
                "axiom-sync-apply-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            Project(root)
        }

        fn read(&self, path: &str) -> Option<String> {
            fs::read_to_string(self.0.join(path)).ok()
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn change(path: &str, after: &str) -> Change {
        Change { path: path.into(), before: None, after: after.into() }
    }

    fn codes(problems: &[Diagnostic]) -> Vec<&str> {
        problems.iter().map(|problem| &*problem.code).collect()
    }

    #[test]
    fn changes_replace_files_and_make_the_folders_they_need() {
        let project = Project::new();
        fs::write(project.0.join("prices.ax"), "old\n").unwrap();
        let changes = [change("prices.ax", "new\n"), change("journal/2026/03.ax", "item\n")];
        assert!(apply(&project.0, &changes).is_empty());
        assert_eq!(project.read("prices.ax").as_deref(), Some("new\n"));
        assert_eq!(project.read("journal/2026/03.ax").as_deref(), Some("item\n"));
        let leftovers = fs::read_dir(&project.0)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains("axiom-sync"));
        assert_eq!(leftovers.count(), 0, "staged files are renamed away");
    }

    #[test]
    fn a_path_that_is_not_inside_the_project_is_refused_and_the_rest_are_written() {
        let project = Project::new();
        let changes = [change("../outside.ax", "x"), change("", "x"), change("/abs.ax", "x"), change("ok.ax", "y")];
        let problems = apply(&project.0, &changes);
        assert_eq!(codes(&problems), ["sync-path-outside-project"; 3]);
        assert_eq!(problems[0].message, "`../outside.ax` is not a project-relative path");
        assert_eq!(project.read("ok.ax").as_deref(), Some("y"));
        assert!(!project.0.parent().unwrap().join("outside.ax").exists());
    }

    #[test]
    fn a_root_that_is_not_there_is_the_one_problem() {
        let project = Project::new();
        let missing = project.0.join("missing");
        let problems = apply(&missing, &[change("a.ax", "x")]);
        assert_eq!(codes(&problems), ["sync-project-root"]);
        assert!(problems[0].message.starts_with("cannot resolve the project root: "));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_folder_cannot_carry_a_write_out_of_the_project() {
        use std::os::unix::fs::symlink;

        let (project, outside_it) = (Project::new(), Project::new());
        symlink(&outside_it.0, project.0.join("link")).unwrap();
        let changes = [change("link/new-folder/prices.ax", "x"), change("link/prices.ax", "x")];
        let problems = apply(&project.0, &changes);
        assert_eq!(codes(&problems), ["sync-path-outside-project"; 2]);
        assert_eq!(problems[1].message, "`link/prices.ax` leaves the project through a symlink");
        assert!(!outside_it.0.join("new-folder").exists() && !outside_it.0.join("prices.ax").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_target_is_replaced_not_followed() {
        use std::os::unix::fs::symlink;

        let (project, elsewhere) = (Project::new(), Project::new());
        fs::write(elsewhere.0.join("real.ax"), "untouched").unwrap();
        symlink(elsewhere.0.join("real.ax"), project.0.join("journal.ax")).unwrap();
        assert!(apply(&project.0, &[change("journal.ax", "new\n")]).is_empty());
        assert_eq!(project.read("journal.ax").as_deref(), Some("new\n"));
        assert_eq!(elsewhere.read("real.ax").as_deref(), Some("untouched"));
    }

    #[test]
    fn a_folder_that_cannot_be_made_is_one_problem_for_that_change() {
        let project = Project::new();
        fs::write(project.0.join("journal"), "a file, not a folder").unwrap();
        let problems = apply(&project.0, &[change("journal/2026.ax", "x"), change("fine.ax", "y")]);
        assert_eq!(codes(&problems), ["sync-write"]);
        assert_eq!(project.read("fine.ax").as_deref(), Some("y"));
    }
}

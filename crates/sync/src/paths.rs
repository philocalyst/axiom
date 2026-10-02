//! Project-confined paths for local sync inputs and outputs.

use std::fs;
use std::path::{Path, PathBuf};

use axiom_core::Diagnostic;
use axiom_core::glob::{glob, is_pattern};

/// Whether a path names a file beneath the project root on any host platform.
pub(crate) fn is_project_path(path: &str) -> bool {
    use std::path::Component;

    let drive_prefix = path.as_bytes().get(..2).is_some_and(|head| head[0].is_ascii_alphabetic() && head[1] == b':');
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || drive_prefix {
        return false;
    }
    if path.split('/').any(|part| part.is_empty() || matches!(part, "." | "..")) {
        return false;
    }
    Path::new(path).components().all(|part| matches!(part, Component::Normal(_)))
}

/// Matching local files for a declared `read` pattern, in sorted project path
/// order. This does not read contents or run commands.
pub fn matching_paths(root: &Path, pattern: &str) -> Result<Vec<String>, Diagnostic> {
    if !is_project_path(pattern) {
        return Err(Diagnostic::error("sync-read-path", format!("`{pattern}` is not a project-relative path")));
    }
    let root = fs::canonicalize(root)
        .map_err(|error| Diagnostic::error("sync-project-root", format!("could not resolve project root: {error}")))?;
    if !root.is_dir() {
        return Err(Diagnostic::error("sync-project-root", "the project root is not a directory"));
    }

    let mut found = vec![String::new()];
    for part in pattern.split('/').filter(|part| !part.is_empty()) {
        let mut next = Vec::new();
        for folder in &found {
            let joined = |entry: &str| {
                if folder.is_empty() { entry.to_string() } else { format!("{folder}/{entry}") }
            };
            if !is_pattern(part) {
                next.push(joined(part));
                continue;
            }
            let directory = confined(&root, &root.join(folder))?;
            let entries = match fs::read_dir(directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(Diagnostic::error(
                        "sync-read-path",
                        format!("could not read `{folder}` while expanding `{pattern}`: {error}"),
                    ));
                }
            };
            for entry in entries {
                let entry = entry.map_err(|error| {
                    Diagnostic::error(
                        "sync-read-path",
                        format!("could not list `{folder}` while expanding `{pattern}`: {error}"),
                    )
                })?;
                let entry = entry.file_name().to_string_lossy().into_owned();
                if !entry.starts_with('.') && glob(part, &entry) {
                    next.push(joined(&entry));
                }
            }
        }
        found = next;
    }

    let mut files = Vec::new();
    for path in found {
        let canonical = match fs::canonicalize(root.join(&path)) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(Diagnostic::error("sync-read-path", format!("could not resolve `{path}`: {error}")));
            }
        };
        if !canonical.starts_with(&root) {
            return Err(Diagnostic::error("sync-read-path", format!("`{path}` leaves the project through a symlink")));
        }
        if canonical.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn confined(root: &Path, path: &Path) -> Result<PathBuf, Diagnostic> {
    let canonical = match fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(path.to_path_buf()),
        Err(error) => {
            return Err(Diagnostic::error(
                "sync-read-path",
                format!("could not resolve `{}`: {error}", path.display()),
            ));
        }
    };
    if canonical.starts_with(root) {
        Ok(canonical)
    } else {
        Err(Diagnostic::error("sync-read-path", format!("`{}` leaves the project through a symlink", path.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn temp() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "axiom-sync-paths-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn paths_are_project_relative_sorted_and_skip_hidden_files() {
        let root = temp();
        let folder = root.join("imports");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("b.csv"), "b").unwrap();
        fs::write(folder.join("a.csv"), "a").unwrap();
        fs::write(folder.join(".secret.csv"), "hidden").unwrap();
        assert_eq!(matching_paths(&root, "imports/*.csv").unwrap(), ["imports/a.csv", "imports/b.csv"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn path_validation_rejects_parent_absolute_and_windows_drive_paths() {
        for path in [
            "../outside.csv",
            "folder/../outside.csv",
            "./outside.csv",
            "folder/./outside.csv",
            "/outside.csv",
            "C:/outside.csv",
            "d:outside.csv",
        ] {
            assert!(!is_project_path(path), "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn matches_through_a_symlink_cannot_escape_the_project() {
        use std::os::unix::fs::symlink;

        let root = temp();
        let outside = temp();
        let imports = root.join("imports");
        fs::create_dir_all(&imports).unwrap();
        fs::write(outside.join("statement.csv"), "external").unwrap();
        symlink(&outside, imports.join("external")).unwrap();
        symlink(outside.join("statement.csv"), imports.join("outside.csv")).unwrap();
        assert!(matching_paths(&root, "imports/external/*.csv").is_err());
        assert!(matching_paths(&root, "imports/*.csv").is_err());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}

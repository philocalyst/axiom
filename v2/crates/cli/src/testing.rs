//! Helpers shared by tests that touch the file system.

use std::fs;
use std::path::{Path, PathBuf};

/// A scratch folder that removes itself.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// A fresh, empty folder. `name` keeps concurrent tests apart.
    pub fn new(name: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!("axiom-cli-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create scratch folder");
        TempDir { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `text` to `relative`, creating folders as needed.
    pub fn write(&self, relative: &str, text: &str) {
        let file = self.path.join(relative);
        fs::create_dir_all(file.parent().expect("a file has a folder")).expect("create folders");
        fs::write(file, text).expect("write file");
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

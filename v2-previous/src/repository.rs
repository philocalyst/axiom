//! Durable, immutable content-addressed storage for v2 project records.

use crate::{Id, canonical};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const FORMAT_VERSION: u32 = 1;
const MAX_OBJECT_BYTES: u64 = 16 * 1024 * 1024;
const OBJECTS_DIR: &str = "objects";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    version: u32,
    kind: String,
    digest: Id,
    value: T,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnvelope {
    version: u32,
    kind: String,
    digest: Id,
    value: serde_json::Value,
}

/// The byte-level view used by a project to perform semantic verification of all
/// reachable and orphaned objects in the store.
pub(crate) struct RawObject {
    pub(crate) id: Id,
    pub(crate) kind: String,
}

/// A small immutable CAS. Its write/read surface stays crate-private so callers
/// cannot introduce repository object kinds that the project does not validate.
pub(crate) struct Repository {
    objects: PathBuf,
}

impl Repository {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let root = path.as_ref().to_path_buf();
        let root_existed = root.exists();
        fs::create_dir_all(root.join(OBJECTS_DIR))
            .map_err(|e| format!("create project repository: {e}"))?;
        let objects = root.join(OBJECTS_DIR);
        let meta =
            fs::symlink_metadata(&objects).map_err(|e| format!("inspect object store: {e}"))?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("repository objects path must be a directory".into());
        }
        sync_directory(&root)?;
        if !root_existed {
            let parent = root
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            sync_directory(parent)?;
        }
        Ok(Self { objects })
    }

    pub(crate) fn put<T: Serialize>(&self, kind: &str, value: &T) -> Result<Id, String> {
        validate_kind(kind)?;
        let digest = object_id(kind, value);
        let envelope = Envelope {
            version: FORMAT_VERSION,
            kind: kind.to_owned(),
            digest: digest.clone(),
            value,
        };
        let bytes = canonical(&envelope);
        if bytes.len() as u64 > MAX_OBJECT_BYTES {
            return Err(format!(
                "repository object exceeds {MAX_OBJECT_BYTES} bytes"
            ));
        }

        let destination = self.object_path(&digest);
        if destination.exists() {
            self.check_existing(&destination, &digest, kind, &bytes)?;
            return Ok(digest);
        }

        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = self
            .objects
            .join(format!(".tmp-{}-{sequence}", std::process::id()));
        let mut guard = TempPath(Some(temp.clone()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| format!("create temporary repository object: {e}"))?;
        file.write_all(&bytes)
            .map_err(|e| format!("write repository object: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("sync repository object: {e}"))?;
        drop(file);

        // A hard link is an atomic no-replace insertion on the same filesystem.
        // Concurrent writers of the same object either win or validate the winner.
        match fs::hard_link(&temp, &destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.check_existing(&destination, &digest, kind, &bytes)?;
            }
            Err(error) => return Err(format!("insert repository object atomically: {error}")),
        }
        guard.remove();
        sync_directory(&self.objects)?;
        Ok(digest)
    }

    pub(crate) fn get<T: DeserializeOwned + Serialize>(
        &self,
        id: &Id,
        expected_kind: &str,
    ) -> Result<T, String> {
        validate_kind(expected_kind)?;
        let path = self.object_path(id);
        let bytes = read_bounded_regular_file(&path)?;
        let envelope: Envelope<T> = decode_canonical(&bytes)?;
        if envelope.version != FORMAT_VERSION {
            return Err(format!(
                "unsupported repository object version {}",
                envelope.version
            ));
        }
        if envelope.kind != expected_kind {
            return Err(format!(
                "expected object kind {expected_kind}, found {}",
                envelope.kind
            ));
        }
        let actual = object_id(expected_kind, &envelope.value);
        if envelope.digest != actual || &actual != id {
            return Err("repository object digest mismatch".into());
        }
        Ok(envelope.value)
    }

    pub(crate) fn raw(&self, id: &Id) -> Result<RawObject, String> {
        let path = self.object_path(id);
        let bytes = read_bounded_regular_file(&path)?;
        let raw: RawEnvelope = decode_canonical(&bytes)?;
        if raw.version != FORMAT_VERSION {
            return Err(format!(
                "unsupported repository object version {}",
                raw.version
            ));
        }
        validate_kind(&raw.kind)?;
        let actual = object_id(&raw.kind, &raw.value);
        if raw.digest != actual || &actual != id {
            return Err("repository object digest mismatch".into());
        }
        if contains_float(&raw.value) {
            return Err("floating-point values are forbidden in repository objects".into());
        }
        Ok(RawObject {
            id: raw.digest,
            kind: raw.kind,
        })
    }

    pub(crate) fn ids(&self) -> Result<Vec<Id>, String> {
        let entries =
            fs::read_dir(&self.objects).map_err(|e| format!("list repository objects: {e}"))?;
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("read repository directory entry: {e}"))?;
            let file_type = entry
                .file_type()
                .map_err(|e| format!("inspect repository entry: {e}"))?;
            if file_type.is_symlink() {
                return Err(format!(
                    "repository contains a symbolic link: {}",
                    entry.path().display()
                ));
            }
            if !file_type.is_file() {
                return Err(format!(
                    "repository contains a non-file entry: {}",
                    entry.path().display()
                ));
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".tmp-") {
                // An interrupted writer can leave a private staging file. It is
                // not addressable and never participates in project history.
                continue;
            }
            let Some(stem) = name.strip_suffix(".json") else {
                return Err(format!("unexpected repository entry: {name}"));
            };
            let id = stem
                .parse::<Id>()
                .map_err(|e| format!("invalid repository object name {name}: {e}"))?;
            ids.push(id);
        }
        ids.sort();
        Ok(ids)
    }

    pub(crate) fn verify_envelopes(&self) -> Result<Vec<RawObject>, String> {
        self.ids()?.into_iter().map(|id| self.raw(&id)).collect()
    }

    fn object_path(&self, id: &Id) -> PathBuf {
        self.objects.join(format!("{id}.json"))
    }

    fn check_existing(
        &self,
        path: &Path,
        id: &Id,
        kind: &str,
        expected: &[u8],
    ) -> Result<(), String> {
        let bytes = read_bounded_regular_file(path)?;
        let raw: RawEnvelope = decode_canonical(&bytes)?;
        if raw.version != FORMAT_VERSION || raw.kind != kind || raw.digest != *id {
            return Err("an existing repository object conflicts with its content address".into());
        }
        let actual = object_id(kind, &raw.value);
        if actual != *id || bytes != expected {
            return Err(
                "refusing to overwrite a corrupted or conflicting repository object".into(),
            );
        }
        Ok(())
    }
}

struct TempPath(Option<PathBuf>);
impl TempPath {
    fn remove(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}
impl Drop for TempPath {
    fn drop(&mut self) {
        self.remove();
    }
}

fn object_id<T: Serialize>(kind: &str, value: &T) -> Id {
    Id::digest(
        &format!("axiom.v2.repository.{kind}.v{FORMAT_VERSION}"),
        &canonical(value),
    )
}

fn validate_kind(kind: &str) -> Result<(), String> {
    if kind.is_empty()
        || kind.len() > 64
        || !kind
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err("invalid repository object kind".into());
    }
    Ok(())
}

fn decode_canonical<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T, String> {
    let value: T =
        serde_json::from_slice(bytes).map_err(|e| format!("decode repository object: {e}"))?;
    if canonical(&value) != bytes {
        return Err("repository object is not canonical JSON".into());
    }
    Ok(value)
}

fn read_bounded_regular_file(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("read repository object {}: {e}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "repository object is not a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() > MAX_OBJECT_BYTES {
        return Err(format!(
            "repository object exceeds {MAX_OBJECT_BYTES} bytes"
        ));
    }
    let file = File::open(path).map_err(|e| format!("open repository object: {e}"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_OBJECT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read repository object: {e}"))?;
    if bytes.len() as u64 > MAX_OBJECT_BYTES {
        return Err(format!(
            "repository object exceeds {MAX_OBJECT_BYTES} bytes"
        ));
    }
    Ok(bytes)
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| format!("sync repository directory: {e}"))
}

fn contains_float(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Number(number) => number.is_f64(),
        serde_json::Value::Array(items) => items.iter().any(contains_float),
        serde_json::Value::Object(fields) => fields.values().any(contains_float),
        _ => false,
    }
}

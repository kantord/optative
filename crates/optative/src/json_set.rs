use std::fmt::Debug;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::reconcile::{Reconcile, ReconcileErrors};
use crate::{Lifecycle, OptativeSet};

/// [`OptativeSet`] persisted as a jsonl file (`{"key": ..., "value": ...}`
/// per line), so a reload seeds the same "already present" state that
/// [`OptativeSet::with_initial_state`] gives you — no `observe()` needed.
/// Writes are atomic (write-then-rename); there is no cross-process locking.
pub struct OptativeJsonSet<T: Lifecycle> {
    file: PathBuf,
    inner: OptativeSet<T>,
    persist_error: Option<io::Error>,
}

#[derive(serde::Serialize)]
struct WriteEntry<'a, K, V> {
    key: &'a K,
    value: &'a V,
}

#[derive(serde::Deserialize)]
struct ReadEntry<K, V> {
    key: K,
    value: V,
}

fn clear_stale_new_files(file: &Path) -> io::Result<()> {
    let Some(prefix) = file.file_name().and_then(|n| n.to_str()) else {
        return Ok(());
    };
    let prefix = format!("{prefix}.new.");
    let dir = match file.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.starts_with(&prefix))
        {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

impl<T: Lifecycle> OptativeJsonSet<T>
where
    T::Error: Debug,
    T::State: serde::Serialize + serde::de::DeserializeOwned,
{
    /// Load `file` if it exists (an absent file seeds an empty set, same as
    /// a fresh [`OptativeSet::new`]) and persist future changes back to it.
    /// Also discards any `<file>.new.*` sibling: leftovers from a previous
    /// write that crashed before it could be renamed into place.
    pub fn open(file: impl Into<PathBuf>) -> io::Result<Self> {
        let file = file.into();
        clear_stale_new_files(&file)?;
        let items = Self::load(&file)?;
        Ok(Self {
            file,
            inner: OptativeSet::with_initial_state(items),
            persist_error: None,
        })
    }

    fn load(file: &Path) -> io::Result<Vec<(T::Key, T::State)>> {
        let contents = match fs::read_to_string(file) {
            Ok(contents) => contents,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let entry: ReadEntry<T::Key, T::State> = serde_json::from_str(line)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                Ok((entry.key, entry.value))
            })
            .collect()
    }

    // Sorted by key so an unchanged store writes byte-identical output —
    // HashMap iteration order isn't stable across writes otherwise.
    fn persist(&self) -> io::Result<()> {
        if let Some(parent) = self.file.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        let mut lines: Vec<(String, String)> = self
            .inner
            .iter()
            .map(|(key, value)| {
                let sort_key = serde_json::to_string(key).expect("Lifecycle::Key must serialize");
                let line = serde_json::to_string(&WriteEntry { key, value })
                    .expect("Lifecycle::State must serialize");
                (sort_key, line)
            })
            .collect();
        lines.sort_by(|a, b| a.0.cmp(&b.0));

        let mut out = String::new();
        for (_, line) in lines {
            out.push_str(&line);
            out.push('\n');
        }

        let new_file = self.new_file_path();
        fs::write(&new_file, out)?;
        fs::rename(&new_file, &self.file)
    }

    // Per-process, so two writers racing on the same file don't tear each
    // other's write mid-flight (the final rename can still race).
    fn new_file_path(&self) -> PathBuf {
        let mut new_file = self.file.clone().into_os_string();
        new_file.push(format!(".new.{}", std::process::id()));
        PathBuf::from(new_file)
    }

    /// Error from the most recent write, if any. Lifecycle hooks already
    /// ran regardless — the in-memory state is authoritative either way.
    pub fn persist_error(&self) -> Option<&io::Error> {
        self.persist_error.as_ref()
    }

    pub fn get(&self, key: &T::Key) -> Option<&T::State> {
        self.inner.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&T::Key, &T::State)> {
        self.inner.iter()
    }
}

impl<T: Lifecycle> Reconcile<T> for OptativeJsonSet<T>
where
    T::Error: Debug,
    T::State: serde::Serialize + serde::de::DeserializeOwned,
{
    fn reconcile(
        &mut self,
        desired: impl IntoIterator<Item = T>,
        ctx: &mut T::Context,
        output: &mut T::Output,
    ) -> ReconcileErrors<T::Key, T::Error> {
        let errors = self.inner.reconcile(desired, ctx, output);
        self.persist_error = self.persist().err();
        errors
    }
}

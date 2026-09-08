use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

/// Per-thread persisted state. Deliberately tiny: cm never keeps the
/// transcript locally, only the upstream conversation id for each named
/// thread - the Mirror server is the source of truth for history.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StateFile {
    #[serde(default)]
    threads: HashMap<String, ThreadState>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ThreadState {
    pub conversation_id: Option<String>,
}

fn state_path(state_dir: &Path) -> PathBuf {
    state_dir.join("state.json")
}

fn lock_path(state_dir: &Path, thread: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    thread.hash(&mut hasher);
    let key = format!("{:016x}", hasher.finish());
    state_dir.join("locks").join(format!("{key}.lock"))
}

/// Hold an exclusive lock for one conversation thread. Different threads can
/// proceed concurrently while the same thread is serialized.
pub fn lock(state_dir: &Path, thread: &str) -> Result<File> {
    let locks_dir = state_dir.join("locks");
    std::fs::create_dir_all(&locks_dir)
        .with_context(|| format!("creating {}", locks_dir.display()))?;
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(lock_path(state_dir, thread))
        .with_context(|| format!("creating thread lock in {}", locks_dir.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("locking thread '{thread}'"))?;
    Ok(file)
}

impl StateFile {
    pub fn load(state_dir: &Path) -> Result<Self> {
        let path = state_path(state_dir);
        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, state_dir: &Path) -> Result<()> {
        let path = state_path(state_dir);
        let raw = serde_json::to_string_pretty(self)?;

        // Use a uniquely-created tempfile in the same directory. A fixed
        // `.state.json.tmp` name allows concurrent cm processes to overwrite
        // each other's pending writes before rename().
        let mut tmp = NamedTempFile::new_in(state_dir)
            .with_context(|| format!("creating temporary state file in {}", state_dir.display()))?;
        std::io::Write::write_all(&mut tmp, raw.as_bytes())
            .with_context(|| "writing temporary state")?;
        tmp.as_file()
            .sync_all()
            .with_context(|| "flushing temporary state file")?;
        tmp.persist(&path)
            .map_err(|e| anyhow::anyhow!("replacing {}: {}", path.display(), e))?;
        // Ensure the directory entry update is durable where supported.
        if let Ok(dir) = File::open(state_dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    pub fn conversation_id(&self, thread: &str) -> Option<&str> {
        self.threads
            .get(thread)
            .and_then(|t| t.conversation_id.as_deref())
    }

    pub fn set_conversation_id(&mut self, thread: &str, id: String) {
        self.threads
            .entry(thread.to_string())
            .or_default()
            .conversation_id = Some(id);
    }

    pub fn clear(&mut self, thread: &str) {
        self.threads.remove(thread);
    }

    pub fn list_threads(&self) -> Vec<(&str, Option<&str>)> {
        let mut v: Vec<_> = self
            .threads
            .iter()
            .map(|(k, s)| (k.as_str(), s.conversation_id.as_deref()))
            .collect();
        v.sort_by_key(|(k, _)| k.to_string());
        v
    }
}

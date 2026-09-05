use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
        let tmp = state_dir.join(".state.json.tmp");
        let raw = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, raw).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("replacing {}", path.display()))?;
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

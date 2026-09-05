use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::PathBuf;

/// Resolved runtime configuration, assembled from (in increasing priority):
/// 1. a `.env` file (cwd, then the state dir, then next to the binary)
/// 2. real process environment variables
/// 3. CLI flags (applied by the caller after this struct is built)
#[derive(Debug, Clone)]
pub struct Config {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub thread: String,
    pub stream: bool,
    pub state_dir: PathBuf,
}

fn load_dotenv_files(state_dir: &std::path::Path) {
    // Layered, lowest priority first: dotenvy::from_path_iter never
    // overrides variables already set in the process environment, but later
    // calls here can still fill in gaps left by earlier ones since each
    // call only sets what's still unset.
    let _ = dotenvy::from_path(state_dir.join(".env"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let _ = dotenvy::from_path(dir.join(".env"));
        }
    }
    // cwd .env last so it wins among the file-based sources (still never
    // overrides a variable that was already set in the real environment).
    let _ = dotenvy::dotenv();
}

fn default_state_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("CM_STATE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let proj = ProjectDirs::from("net", "eternalvoid", "cm")
        .context("could not determine a home directory for cm's state")?;
    Ok(proj.data_dir().to_path_buf())
}

impl Config {
    pub fn load() -> Result<Self> {
        // We need the state dir before we can look for a .env inside it, but
        // CM_STATE_DIR itself might only be set by a .env in the cwd - so
        // resolve cwd's .env first with a throwaway pass, then the real one.
        let _ = dotenvy::dotenv();
        let state_dir = default_state_dir()?;
        std::fs::create_dir_all(&state_dir)
            .with_context(|| format!("creating state dir {}", state_dir.display()))?;
        load_dotenv_files(&state_dir);

        let base_url = std::env::var("CM_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:8799".to_string())
            .trim_end_matches('/')
            .to_string();
        let api_key = std::env::var("CM_API_KEY").ok().filter(|s| !s.is_empty());
        let model = std::env::var("CM_MODEL").unwrap_or_else(|_| "auto".to_string());
        let thread = std::env::var("CM_THREAD").unwrap_or_else(|_| "default".to_string());
        let stream = std::env::var("CM_STREAM")
            .map(|v| !matches!(v.as_str(), "0" | "false" | "no"))
            .unwrap_or(true);

        Ok(Config {
            base_url,
            api_key,
            model,
            thread,
            stream,
            state_dir,
        })
    }
}

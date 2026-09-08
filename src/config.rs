use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::PathBuf;

/// Runtime prompt templates loaded from external configuration.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Prompts {
    pub exec: String,
}

impl Prompts {
    pub fn load(state_dir: &std::path::Path) -> Result<Self> {
        let path = state_dir.join("prompts.yaml");
        if let Ok(raw) = std::fs::read_to_string(&path) {
            return serde_yaml::from_str(&raw)
                .with_context(|| format!("parsing {}", path.display()));
        }
        serde_yaml::from_str(DEFAULT_PROMPTS_YAML)
            .context("parsing bundled prompts.yaml")
    }
}

const DEFAULT_PROMPTS_YAML: &str = include_str!("../prompts.yaml");

/// Resolved runtime configuration.
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

fn non_empty(v: std::result::Result<String, std::env::VarError>) -> Option<String> {
    v.ok().filter(|s| !s.trim().is_empty())
}

/// Resolves which bearer token to send to Mirror, in priority order:
/// 1. `CM_API_KEY` - cm-specific override, in case you want cm to use a
///    different key than the rest of Mirror.
/// 2. `MIRROR_API_KEY` - the same env var the Mirror server itself reads
///    (`apps/server/src/security.ts` `configuredApiKeys()`), so a `.env`
///    shared between the server and cm just works with no duplication.
/// 3. `MIRROR_API_KEYS` - Mirror's comma-separated multi-key variant; any
///    one of the listed keys is valid, so we just take the first non-empty
///    entry.
/// 4. `OPENAI_API_KEY` - last-resort fallback for setups that only ever
///    configured an OpenAI-style key and never bothered with a
///    Mirror-specific one.
fn resolve_api_key() -> Option<String> {
    non_empty(std::env::var("CM_API_KEY"))
        .or_else(|| non_empty(std::env::var("MIRROR_API_KEY")))
        .or_else(|| {
            non_empty(std::env::var("MIRROR_API_KEYS")).and_then(|raw| {
                raw.split(',')
                    .map(|s| s.trim())
                    .find(|s| !s.is_empty())
                    .map(|s| s.to_string())
            })
        })
        .or_else(|| non_empty(std::env::var("OPENAI_API_KEY")))
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
        let api_key = resolve_api_key();
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

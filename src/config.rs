use serde::{Deserialize, Serialize};
use crate::agent;

// ── Persistent config ─────────────────────────────────────────────────────────

// provider index: 0=Groq  1=OpenRouter  2=Local/Self-hosted
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PointerConfig {
    #[serde(default)]
    pub provider:            u8,
    // Groq
    pub groq_api_key:        String,
    pub groq_model:          String,
    pub groq_url:            String,
    // OpenRouter
    #[serde(default)]
    pub openrouter_api_key:  String,
    #[serde(default = "default_or_model")]
    pub openrouter_model:    String,
    #[serde(default = "default_or_url")]
    pub openrouter_url:      String,
    // Local / self-hosted
    pub local_base_url:      String,
    pub local_api_key:       String,
    pub local_model:         String,
}

fn default_or_model() -> String { agent::OPENROUTER_MODEL_DEFAULT.into() }
fn default_or_url()   -> String { agent::OPENROUTER_URL_DEFAULT.into() }

impl Default for PointerConfig {
    fn default() -> Self {
        Self {
            provider:           0,
            groq_api_key:       std::env::var("GROQ_API_KEY").unwrap_or_default(),
            groq_model:         std::env::var("GROQ_MODEL")
                                    .unwrap_or_else(|_| agent::GROQ_MODEL_DEFAULT.into()),
            groq_url:           std::env::var("GROQ_URL")
                                    .unwrap_or_else(|_| agent::GROQ_URL_DEFAULT.into()),
            openrouter_api_key: std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            openrouter_model:   std::env::var("OPENROUTER_MODEL")
                                    .unwrap_or_else(|_| agent::OPENROUTER_MODEL_DEFAULT.into()),
            openrouter_url:     agent::OPENROUTER_URL_DEFAULT.into(),
            local_base_url:     String::new(),
            local_api_key:      "EMPTY".into(),
            local_model:        String::new(),
        }
    }
}

impl PointerConfig {
    pub fn effective_api_key(&self) -> &str {
        match self.provider {
            1 => &self.openrouter_api_key,
            2 => &self.local_api_key,
            _ => &self.groq_api_key,
        }
    }
    pub fn effective_model(&self) -> &str {
        match self.provider {
            1 => &self.openrouter_model,
            2 => &self.local_model,
            _ => &self.groq_model,
        }
    }
    pub fn effective_api_url(&self) -> String {
        match self.provider {
            1 => self.openrouter_url.clone(),
            2 => format!("{}/chat/completions",
                         self.local_base_url.trim_end_matches('/')),
            _ => self.groq_url.clone(),
        }
    }
}

pub fn config_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    std::path::PathBuf::from(home).join(".pointer_config.json")
}

pub fn load_config() -> PointerConfig {
    if let Ok(data) = std::fs::read_to_string(config_path()) {
        if let Ok(cfg) = serde_json::from_str(&data) { return cfg; }
    }
    PointerConfig::default()
}

pub fn save_config(cfg: &PointerConfig) {
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(config_path(), json);
    }
}

// ── Model-settings field pointers (set once during UI build) ──────────────────

pub struct MsPtrs {
    pub provider_popup: usize,
    // Groq
    pub groq_api_fld:   usize,
    pub groq_mdl_fld:   usize,
    pub groq_url_fld:   usize,
    // OpenRouter
    pub or_api_fld:     usize,
    pub or_mdl_fld:     usize,
    pub or_url_fld:     usize,
    // Local
    pub loc_url_fld:    usize,
    pub loc_key_fld:    usize,
    pub loc_mdl_fld:    usize,
    // Containers
    pub groq_box:       usize,
    pub or_box:         usize,
    pub loc_box:        usize,
    pub status_lbl:     usize,
}

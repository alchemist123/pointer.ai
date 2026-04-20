use serde::{Deserialize, Serialize};
use crate::agent;

// ── Persistent config ─────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PointerConfig {
    pub provider:       u8,   // 0 = Groq, 1 = Local
    pub groq_api_key:   String,
    pub groq_model:     String,
    pub groq_url:       String,
    pub local_base_url: String,
    pub local_api_key:  String,
    pub local_model:    String,
}

impl Default for PointerConfig {
    fn default() -> Self {
        Self {
            provider:       0,
            groq_api_key:   std::env::var("GROQ_API_KEY").unwrap_or_default(),
            groq_model:     std::env::var("GROQ_MODEL")
                                .unwrap_or_else(|_| agent::GROQ_MODEL_DEFAULT.into()),
            groq_url:       std::env::var("GROQ_URL")
                                .unwrap_or_else(|_| agent::GROQ_URL_DEFAULT.into()),
            local_base_url: String::new(),
            local_api_key:  "EMPTY".into(),
            local_model:    String::new(),
        }
    }
}

impl PointerConfig {
    pub fn effective_api_key(&self) -> &str {
        if self.provider == 0 { &self.groq_api_key } else { &self.local_api_key }
    }
    pub fn effective_model(&self) -> &str {
        if self.provider == 0 { &self.groq_model } else { &self.local_model }
    }
    pub fn effective_api_url(&self) -> String {
        if self.provider == 0 {
            self.groq_url.clone()
        } else {
            format!("{}/chat/completions", self.local_base_url.trim_end_matches('/'))
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

// ── Model-settings field pointers (set once during build) ─────────────────────

pub struct MsPtrs {
    pub provider_seg: usize,
    pub groq_api_fld: usize,
    pub groq_mdl_fld: usize,
    pub groq_url_fld: usize,
    pub loc_url_fld:  usize,
    pub loc_key_fld:  usize,
    pub loc_mdl_fld:  usize,
    pub groq_box:     usize,
    pub loc_box:      usize,
    pub status_lbl:   usize,
}

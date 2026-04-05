use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub root: String,
    pub query: String,
    pub limit: usize,
    #[serde(default)]
    pub include_hidden: bool,
    #[serde(default = "default_use_cache")]
    pub use_cache: bool,
    #[serde(default = "default_cache_ttl_secs")]
    pub cache_ttl_secs: u64,
    #[serde(default)]
    pub rebuild_cache: bool,
}

fn default_use_cache() -> bool {
    true
}

fn default_cache_ttl_secs() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub recency_weight: f64,
    pub git_modified_weight: f64,
    pub git_untracked_weight: f64,
    pub extension_weight: f64,
    pub typo_weight: f64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            recency_weight: 1.35,
            git_modified_weight: 0.35,
            git_untracked_weight: 0.25,
            extension_weight: 0.20,
            typo_weight: 0.80,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub path: String,
    pub score: f64,
    pub matched_indices: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepResult {
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub text: String,
}

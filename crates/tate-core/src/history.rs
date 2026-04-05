use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryEntry {
    pub uses: u32,
    pub last_opened_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryStore {
    pub files: FxHashMap<String, HistoryEntry>,
}

impl HistoryStore {
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(contents) => serde_json::from_str::<HistoryStore>(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let payload = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        fs::write(path, payload)
    }

    pub fn touch_file(&mut self, rel_path: &str) {
        let entry = self.files.entry(rel_path.to_string()).or_default();
        entry.uses = entry.uses.saturating_add(1);
        entry.last_opened_unix = Utc::now().timestamp();
    }

    pub fn recency_score(&self, rel_path: &str) -> f64 {
        let Some(entry) = self.files.get(rel_path) else {
            return 0.0;
        };

        let uses_boost = (entry.uses as f64).ln_1p() * 0.20;
        let age_hours = Utc::now()
            .signed_duration_since(unix_to_dt(entry.last_opened_unix))
            .num_hours()
            .max(0) as f64;

        let recency_decay = 1.0 / (1.0 + age_hours / 24.0);
        uses_boost + recency_decay
    }
}

fn unix_to_dt(ts: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now)
}

pub fn default_history_path() -> PathBuf {
    let root = std::env::var("XDG_STATE_HOME")
        .or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.local/state")))
        .unwrap_or_else(|_| ".".to_string());

    PathBuf::from(root).join("smart-fuzzy-finder").join("history.json")
}

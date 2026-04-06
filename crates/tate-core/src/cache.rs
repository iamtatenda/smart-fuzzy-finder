use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use chrono::Utc;
use rustc_hash::FxHasher;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub use_cache: bool,
    pub ttl_secs: u64,
    pub rebuild: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            use_cache: true,
            ttl_secs: 30,
            rebuild: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexCachePayload {
    version: u32,
    root: String,
    include_hidden: bool,
    created_unix: i64,
    files: Vec<String>,
}

const CACHE_VERSION: u32 = 1;

pub fn load_index_cache(root: &Path, include_hidden: bool, ttl_secs: u64) -> Option<Vec<String>> {
    load_index_cache_from_dir(&default_cache_dir(), root, include_hidden, ttl_secs)
}

pub fn save_index_cache(
    root: &Path,
    include_hidden: bool,
    files: &[String],
) -> std::io::Result<()> {
    save_index_cache_to_dir(&default_cache_dir(), root, include_hidden, files)
}

fn load_index_cache_from_dir(
    cache_dir: &Path,
    root: &Path,
    include_hidden: bool,
    ttl_secs: u64,
) -> Option<Vec<String>> {
    let path = index_cache_path_in(cache_dir, root, include_hidden);
    let contents = fs::read_to_string(path).ok()?;
    let payload = serde_json::from_str::<IndexCachePayload>(&contents).ok()?;

    if payload.version != CACHE_VERSION || payload.include_hidden != include_hidden {
        return None;
    }

    if payload.root != normalize_root(root) {
        return None;
    }

    if ttl_secs > 0 {
        let age = Utc::now().timestamp().saturating_sub(payload.created_unix);
        if age > ttl_secs as i64 {
            return None;
        }
    }

    Some(payload.files)
}

fn save_index_cache_to_dir(
    cache_dir: &Path,
    root: &Path,
    include_hidden: bool,
    files: &[String],
) -> std::io::Result<()> {
    let path = index_cache_path_in(cache_dir, root, include_hidden);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let payload = IndexCachePayload {
        version: CACHE_VERSION,
        root: normalize_root(root),
        include_hidden,
        created_unix: Utc::now().timestamp(),
        files: files.to_vec(),
    };

    let body = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    fs::write(path, body)
}

fn normalize_root(root: &Path) -> String {
    root.to_string_lossy().replace('\\', "/")
}

/// Build a collision-resistant cache file path.
///
/// The key is a sanitised prefix of the path (up to 80 chars) followed by an
/// underscore and 16 hex digits of FxHash of the full path string.  This
/// prevents collisions between repos whose paths share a long common prefix
/// while keeping the filename human-readable.
pub(crate) fn index_cache_path_in(cache_dir: &Path, root: &Path, include_hidden: bool) -> PathBuf {
    let root_component = normalize_root(root);

    let mut hasher = FxHasher::default();
    root_component.hash(&mut hasher);
    let hash = hasher.finish();

    let mut key = sanitize_key(&root_component);
    if key.len() > 80 {
        key.truncate(80);
    }
    key.push('_');
    key.push_str(&format!("{hash:016x}"));
    key.push_str(if include_hidden {
        "_hidden"
    } else {
        "_visible"
    });

    cache_dir
        .join("smart-fuzzy-finder")
        .join("index")
        .join(format!("{key}.json"))
}

fn sanitize_key(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "root".to_string()
    } else {
        out
    }
}

fn default_cache_dir() -> PathBuf {
    let root = std::env::var("XDG_CACHE_HOME")
        .or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.cache")))
        .unwrap_or_else(|_| ".".to_string());

    PathBuf::from(root)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{index_cache_path_in, load_index_cache_from_dir, save_index_cache_to_dir};

    #[test]
    fn cache_roundtrip_returns_saved_files() {
        let cache_dir = tempdir().expect("temp cache dir");
        let root_dir = tempdir().expect("temp root dir");
        let root = root_dir.path();

        let files = vec![
            "src/main.rs".to_string(),
            "lua/smart_fuzzy_finder/init.lua".to_string(),
        ];

        save_index_cache_to_dir(cache_dir.path(), root, false, &files).expect("save cache");
        let loaded =
            load_index_cache_from_dir(cache_dir.path(), root, false, 60).expect("load cache");

        assert_eq!(loaded, files);
    }

    #[test]
    fn cache_rejects_hidden_mode_mismatch() {
        let cache_dir = tempdir().expect("temp cache dir");
        let root = Path::new("/repo/sample");
        let files = vec!["src/lib.rs".to_string()];

        save_index_cache_to_dir(cache_dir.path(), root, false, &files).expect("save cache");
        let loaded = load_index_cache_from_dir(cache_dir.path(), root, true, 60);

        assert!(loaded.is_none());
    }

    #[test]
    fn cache_rejects_expired_ttl() {
        let cache_dir = tempdir().expect("temp cache dir");
        let root_dir = tempdir().expect("temp root dir");
        let root = root_dir.path();

        let files = vec!["src/main.rs".to_string()];
        save_index_cache_to_dir(cache_dir.path(), root, false, &files).expect("save cache");

        // Overwrite created_unix with Unix epoch (way in the past).
        let cache_path = index_cache_path_in(cache_dir.path(), root, false);
        let content = fs::read_to_string(&cache_path).expect("read cache file");
        let mut payload: serde_json::Value =
            serde_json::from_str(&content).expect("parse cache JSON");
        payload["created_unix"] = serde_json::json!(0);
        fs::write(&cache_path, serde_json::to_string(&payload).unwrap())
            .expect("write modified cache");

        // TTL of 60 s but the file is from the epoch — must be rejected.
        let loaded = load_index_cache_from_dir(cache_dir.path(), root, false, 60);
        assert!(loaded.is_none(), "expired cache entry must be rejected");
    }

    #[test]
    fn cache_key_differs_for_long_shared_prefix() {
        // Two paths that share the first 80+ characters must get different keys.
        let prefix = "a".repeat(100);
        let path_a = format!("{prefix}/projectA");
        let path_b = format!("{prefix}/projectB");
        let root_a = Path::new(&path_a);
        let root_b = Path::new(&path_b);

        let cache_dir = tempdir().expect("temp cache dir");
        let key_a = index_cache_path_in(cache_dir.path(), root_a, false);
        let key_b = index_cache_path_in(cache_dir.path(), root_b, false);

        assert_ne!(
            key_a, key_b,
            "different root paths must produce different cache keys"
        );
    }
}

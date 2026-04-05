use std::fs::File;
use std::io::Read;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use ignore::{WalkBuilder, WalkState};

use crate::cache::{load_index_cache, save_index_cache, CacheConfig};
use crate::git::collect_git_signals;
use crate::history::{default_history_path, HistoryStore};
use crate::types::{GrepResult, MatchResult, SearchConfig, SearchRequest};

#[derive(Debug)]
struct ScoredCandidate {
    path: String,
    score: f64,
    matched_indices: Vec<usize>,
}

pub fn build_index(root: &Path, include_hidden: bool) -> Vec<String> {
    let mut builder = WalkBuilder::new(root);

    builder
        .hidden(!include_hidden)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .follow_links(false)
        .standard_filters(true);

    let (tx, rx) = mpsc::channel::<String>();
    let root_buf = root.to_path_buf();

    builder.build_parallel().run(|| {
        let tx = tx.clone();
        let root_buf = root_buf.clone();
        Box::new(move |entry| {
            let entry = match entry {
                Ok(e) => e,
                // Skip unreadable entries (permission errors, broken symlinks,
                // etc.) — the same behaviour as the previous single-threaded
                // walker which used `.flatten()`.
                Err(_) => return WalkState::Continue,
            };
            let path = entry.path();
            if !path.is_file() {
                return WalkState::Continue;
            }
            let Ok(rel) = path.strip_prefix(&root_buf) else {
                return WalkState::Continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if !rel.is_empty() {
                // The only failure mode is a disconnected receiver, which
                // only happens if the calling thread panics.  In that case
                // dropping further results is correct.
                let _ = tx.send(rel);
            }
            WalkState::Continue
        })
    });

    // All thread-local senders are dropped when run() returns; drop
    // the original so the receiver sees EOF.
    drop(tx);
    rx.into_iter().collect()
}

pub fn search(request: &SearchRequest, config: &SearchConfig) -> Vec<MatchResult> {
    let root = PathBuf::from(&request.root);
    let query = request.query.trim();

    if query.is_empty() {
        return Vec::new();
    }

    let history = HistoryStore::load(&default_history_path());
    let git = collect_git_signals(&root);
    let cache_cfg = CacheConfig {
        use_cache: request.use_cache,
        ttl_secs: request.cache_ttl_secs,
        rebuild: request.rebuild_cache,
    };
    let index = indexed_files(&root, request.include_hidden, &cache_cfg);

    let mut scored = Vec::new();
    for rel_path in index {
        let Some((fuzzy_score, matched_indices, typo_bonus)) = fuzzy_score(query, &rel_path) else {
            continue;
        };

        let recency = history.recency_score(&rel_path) * config.recency_weight;
        let git_boost = if git.modified.contains(&rel_path) {
            config.git_modified_weight
        } else if git.untracked.contains(&rel_path) {
            config.git_untracked_weight
        } else {
            0.0
        };

        let extension_boost = extension_score(query, &rel_path) * config.extension_weight;
        let score =
            fuzzy_score + recency + git_boost + extension_boost + typo_bonus * config.typo_weight;

        scored.push(ScoredCandidate {
            path: rel_path,
            score,
            matched_indices,
        });
    }

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.len().cmp(&b.path.len()))
            .then_with(|| a.path.cmp(&b.path))
    });

    scored
        .into_iter()
        .take(request.limit)
        .map(|item| MatchResult {
            path: item.path,
            score: item.score,
            matched_indices: item.matched_indices,
        })
        .collect()
}

pub fn record_open(rel_path: &str) -> std::io::Result<()> {
    let history_path = default_history_path();
    let mut history = HistoryStore::load(&history_path);
    history.touch_file(rel_path);
    history.save(&history_path)
}

pub fn grep_project(
    root: &Path,
    pattern: &str,
    limit: usize,
    include_hidden: bool,
    cache: &CacheConfig,
) -> Vec<GrepResult> {
    let needle = pattern.trim();
    if needle.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    for rel_path in indexed_files(root, include_hidden, cache) {
        if results.len() >= limit {
            break;
        }

        let abs = root.join(&rel_path);
        if is_probably_binary_file(&abs) {
            continue;
        }

        let file = match File::open(&abs) {
            Ok(file) => file,
            Err(_) => continue,
        };

        let reader = BufReader::new(file);
        for (line_idx, line) in reader.lines().enumerate() {
            if results.len() >= limit {
                break;
            }

            let Ok(text) = line else {
                continue;
            };

            if let Some(col) = fuzzy_line_match(&text, needle) {
                results.push(GrepResult {
                    path: rel_path.clone(),
                    line: line_idx + 1,
                    column: col + 1,
                    text,
                });
            }
        }
    }

    results
}

fn indexed_files(root: &Path, include_hidden: bool, cache: &CacheConfig) -> Vec<String> {
    if cache.use_cache && !cache.rebuild {
        if let Some(cached) = load_index_cache(root, include_hidden, cache.ttl_secs) {
            return cached;
        }
    }

    let files = build_index(root, include_hidden);
    if cache.use_cache {
        let _ = save_index_cache(root, include_hidden, &files);
    }
    files
}

fn is_probably_binary_file(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };

    let mut buf = [0u8; 4096];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };

    buf[..n].contains(&0)
}

fn fuzzy_line_match(line: &str, query: &str) -> Option<usize> {
    let lc_line = line.to_ascii_lowercase();
    let lc_query = query.to_ascii_lowercase();

    if let Some(idx) = lc_line.find(&lc_query) {
        return Some(idx);
    }

    if !lc_line.is_ascii() || !lc_query.is_ascii() {
        return None;
    }

    let mut best: Option<(usize, usize)> = None;
    let q_len = lc_query.len();
    if q_len < 3 || q_len > lc_line.len() {
        return None;
    }

    for start in 0..=(lc_line.len() - q_len) {
        let window = &lc_line[start..start + q_len];
        let dist = levenshtein(window, &lc_query);
        if dist <= 1 {
            match best {
                Some((best_dist, _)) if best_dist <= dist => {}
                _ => best = Some((dist, start)),
            }
        }
    }

    best.map(|(_, idx)| idx)
}

fn extension_score(query: &str, path: &str) -> f64 {
    let query_ext = query.split('.').next_back().unwrap_or("");
    let path_ext = path.split('.').next_back().unwrap_or("");

    if !query.contains('.') || query_ext.is_empty() || path_ext.is_empty() {
        return 0.0;
    }

    if path_ext.eq_ignore_ascii_case(query_ext) {
        0.25
    } else {
        0.0
    }
}

fn fuzzy_score(query: &str, candidate: &str) -> Option<(f64, Vec<usize>, f64)> {
    let mut q = query.to_ascii_lowercase();
    q.retain(|c| !c.is_whitespace());
    if q.is_empty() {
        return None;
    }

    let c = candidate.to_ascii_lowercase();
    let q_chars: Vec<char> = q.chars().collect();
    let c_chars: Vec<char> = c.chars().collect();

    let mut positions = Vec::with_capacity(q_chars.len());
    let mut q_i = 0usize;
    let mut last_match = None;
    let mut streak = 0usize;
    let mut score = 0.0;

    for (idx, &ch) in c_chars.iter().enumerate() {
        if q_i >= q_chars.len() {
            break;
        }

        if ch != q_chars[q_i] {
            continue;
        }

        positions.push(idx);
        score += 1.0;

        let prev_ch = idx.checked_sub(1).and_then(|i| c_chars.get(i)).copied();
        if idx == 0 || matches!(prev_ch, Some('/' | '_' | '-')) {
            score += 0.35;
        }

        if let Some(prev) = last_match {
            if idx == prev + 1 {
                streak += 1;
                score += 0.30 + (streak as f64 * 0.05);
            } else {
                streak = 0;
            }
        }

        last_match = Some(idx);
        q_i += 1;
    }

    if q_i != q_chars.len() {
        return None;
    }

    let compactness = q_chars.len() as f64 / (c_chars.len().max(1) as f64);
    score += compactness * 1.2;

    let mut typo_bonus = 0.0;
    let file_name = candidate.rsplit('/').next().unwrap_or(candidate);
    let d = normalized_levenshtein(&q, &file_name.to_ascii_lowercase());
    if d <= 0.45 {
        typo_bonus = (0.45 - d).max(0.0);
    }

    Some((score, positions, typo_bonus))
}

fn normalized_levenshtein(a: &str, b: &str) -> f64 {
    let max_len = a.len().max(b.len()).max(1) as f64;
    levenshtein(a, b) as f64 / max_len
}

fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.chars().count();
    }
    if b.is_empty() {
        return a.chars().count();
    }

    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0; b_chars.len() + 1];

    for (i, a_ch) in a.chars().enumerate() {
        curr[0] = i + 1;

        for (j, b_ch) in b_chars.iter().enumerate() {
            let cost = if a_ch == *b_ch { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }

        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_chars.len()]
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::cache::CacheConfig;
    use crate::types::{SearchConfig, SearchRequest};

    use super::{build_index, extension_score, fuzzy_score, grep_project, search};

    #[test]
    fn search_handles_typo_query() {
        let root = tempdir().expect("temp root");
        fs::create_dir_all(root.path().join("src")).expect("create src");
        fs::write(root.path().join("src/history.rs"), "pub fn touch() {}\n")
            .expect("write history file");
        fs::write(root.path().join("src/finder.rs"), "pub fn find() {}\n")
            .expect("write finder file");

        let request = SearchRequest {
            root: root.path().to_string_lossy().to_string(),
            query: "histry".to_string(),
            limit: 10,
            include_hidden: false,
            use_cache: false,
            cache_ttl_secs: 30,
            rebuild_cache: false,
        };

        let results = search(&request, &SearchConfig::default());
        assert!(!results.is_empty(), "expected at least one search result");
        assert_eq!(results[0].path, "src/history.rs");
    }

    #[test]
    fn grep_skips_binary_files() {
        let root = tempdir().expect("temp root");
        fs::write(root.path().join("notes.txt"), "hello needle world\n").expect("write text file");
        fs::write(root.path().join("blob.bin"), b"\0\x01needle\x02").expect("write binary file");

        let cache = CacheConfig {
            use_cache: false,
            ttl_secs: 30,
            rebuild: false,
        };

        let results = grep_project(root.path(), "needle", 50, false, &cache);
        assert_eq!(results.len(), 1, "expected only text file match");
        assert_eq!(results[0].path, "notes.txt");
    }

    #[test]
    fn fuzzy_score_exact_match_scores_higher_than_scattered() {
        // "finder" exactly in filename beats scattered characters
        let exact = fuzzy_score("finder", "src/finder.rs");
        let scattered = fuzzy_score("finder", "src/facade_index_renderer.rs");
        assert!(exact.is_some());
        assert!(scattered.is_some());
        assert!(
            exact.unwrap().0 > scattered.unwrap().0,
            "exact substring match should score higher than scattered match"
        );
    }

    #[test]
    fn fuzzy_score_returns_none_when_not_subsequence() {
        let result = fuzzy_score("xyz", "abcdef");
        assert!(result.is_none(), "non-subsequence query must return None");
    }

    #[test]
    fn fuzzy_score_empty_query_returns_none() {
        let result = fuzzy_score("   ", "src/main.rs");
        assert!(result.is_none(), "whitespace-only query must return None");
    }

    #[test]
    fn fuzzy_score_boundary_bonus_for_path_separator() {
        // 'f' at the start of a path component gets a boundary bonus
        let at_boundary = fuzzy_score("f", "src/finder.rs");
        let mid_word = fuzzy_score("i", "src/finder.rs");
        assert!(at_boundary.is_some() && mid_word.is_some());
        // 'f' is right after '/', so it gets +0.35 boundary bonus
        assert!(
            at_boundary.unwrap().0 > mid_word.unwrap().0,
            "character at path boundary should score higher"
        );
    }

    #[test]
    fn extension_score_matches_same_extension() {
        let score = extension_score("main.rs", "src/main.rs");
        assert!(
            score > 0.0,
            "matching extensions should give positive score"
        );
    }

    #[test]
    fn extension_score_no_bonus_for_different_extension() {
        let score = extension_score("main.rs", "src/main.py");
        assert_eq!(score, 0.0, "different extensions should give zero score");
    }

    #[test]
    fn extension_score_no_bonus_for_query_without_dot() {
        let score = extension_score("main", "src/main.rs");
        assert_eq!(score, 0.0, "query without extension should give zero score");
    }

    #[test]
    fn build_index_finds_all_files() {
        let root = tempdir().expect("temp root");
        fs::create_dir_all(root.path().join("src")).expect("create src");
        fs::write(root.path().join("src/main.rs"), "fn main() {}").expect("write main");
        fs::write(root.path().join("README.md"), "# Project").expect("write readme");

        let mut files = build_index(root.path(), false);
        files.sort();

        assert_eq!(files, vec!["README.md", "src/main.rs"]);
    }

    #[test]
    fn search_returns_empty_for_empty_query() {
        let root = tempdir().expect("temp root");
        fs::write(root.path().join("main.rs"), "fn main() {}").expect("write file");

        let request = SearchRequest {
            root: root.path().to_string_lossy().to_string(),
            query: "".to_string(),
            limit: 10,
            include_hidden: false,
            use_cache: false,
            cache_ttl_secs: 30,
            rebuild_cache: false,
        };

        let results = search(&request, &SearchConfig::default());
        assert!(results.is_empty(), "empty query should return no results");
    }
}

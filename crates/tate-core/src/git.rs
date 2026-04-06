use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rustc_hash::FxHashSet;

#[derive(Debug, Default, Clone)]
pub struct GitSignals {
    pub modified: FxHashSet<String>,
    pub untracked: FxHashSet<String>,
}

/// In-process cache: (root, signals, captured_at).
/// Avoids re-running `git status` on every keystroke in the Neovim picker.
static GIT_SIGNAL_CACHE: Mutex<Option<(PathBuf, GitSignals, Instant)>> = Mutex::new(None);
const GIT_CACHE_TTL: Duration = Duration::from_secs(2);

pub fn collect_git_signals(root: &Path) -> GitSignals {
    // Check cache under a short-lived lock.
    {
        match GIT_SIGNAL_CACHE.lock() {
            Ok(guard) => {
                if let Some((ref cached_root, ref signals, ref ts)) = *guard {
                    if cached_root == root && ts.elapsed() < GIT_CACHE_TTL {
                        return signals.clone();
                    }
                }
            }
            // Poisoned mutex means a previous holder panicked.  Fall through
            // to a fresh git-status call rather than propagating the poison.
            Err(_) => {}
        }
    }

    let signals = run_git_status(root);

    // Store in cache; ignore a poisoned mutex here too.
    if let Ok(mut guard) = GIT_SIGNAL_CACHE.lock() {
        *guard = Some((root.to_path_buf(), signals.clone(), Instant::now()));
    }

    signals
}

fn run_git_status(root: &Path) -> GitSignals {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("status")
        .arg("--porcelain")
        .output();

    let Ok(output) = output else {
        return GitSignals::default();
    };

    if !output.status.success() {
        return GitSignals::default();
    }

    let mut signals = GitSignals::default();
    let stdout = String::from_utf8_lossy(&output.stdout);

    for raw_line in stdout.lines() {
        // Porcelain v1: "XY path" — X and Y are always single ASCII bytes,
        // byte 2 is always a space.  Validate before slicing to stay safe
        // even if an unusual locale emits non-ASCII status characters.
        let bytes = raw_line.as_bytes();
        if bytes.len() < 4 || !bytes[0].is_ascii() || !bytes[1].is_ascii() || bytes[2] != b' ' {
            continue;
        }

        let status = &raw_line[0..2];
        let mut path = raw_line[3..].trim();

        if let Some((_, rhs)) = path.split_once(" -> ") {
            path = rhs.trim();
        }

        if path.is_empty() {
            continue;
        }

        match status {
            "??" => {
                signals.untracked.insert(path.to_string());
            }
            _ => {
                signals.modified.insert(path.to_string());
            }
        }
    }

    signals
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use tempfile::tempdir;

    use super::collect_git_signals;

    fn git(args: &[&str], dir: &std::path::Path) {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
    }

    #[test]
    fn git_signals_detects_untracked_file() {
        let dir = tempdir().expect("temp dir");
        git(&["init"], dir.path());
        git(&["config", "user.email", "test@example.com"], dir.path());
        git(&["config", "user.name", "Test"], dir.path());

        fs::write(dir.path().join("new.rs"), "fn foo() {}").expect("write file");

        let signals = collect_git_signals(dir.path());
        assert!(
            signals.untracked.contains("new.rs"),
            "untracked file should appear in signals"
        );
        assert!(
            signals.modified.is_empty(),
            "no committed files yet so modified should be empty"
        );
    }

    #[test]
    fn git_signals_detects_modified_file() {
        let dir = tempdir().expect("temp dir");
        git(&["init"], dir.path());
        git(&["config", "user.email", "test@example.com"], dir.path());
        git(&["config", "user.name", "Test"], dir.path());

        fs::write(dir.path().join("existing.rs"), "fn foo() {}").expect("write file");
        git(&["add", "existing.rs"], dir.path());
        git(&["commit", "-m", "initial commit"], dir.path());

        // Modify the committed file.
        fs::write(dir.path().join("existing.rs"), "fn bar() {}").expect("modify file");

        let signals = collect_git_signals(dir.path());
        assert!(
            signals.modified.contains("existing.rs"),
            "modified file should appear in signals"
        );
    }

    #[test]
    fn git_signals_empty_in_non_git_dir() {
        let dir = tempdir().expect("temp dir");
        // No `git init` — not a git repo.
        let signals = collect_git_signals(dir.path());
        assert!(
            signals.modified.is_empty() && signals.untracked.is_empty(),
            "non-git directory should return empty signals"
        );
    }
}

use std::path::Path;
use std::process::Command;

use rustc_hash::FxHashSet;

#[derive(Debug, Default, Clone)]
pub struct GitSignals {
	pub modified: FxHashSet<String>,
	pub untracked: FxHashSet<String>,
}

pub fn collect_git_signals(root: &Path) -> GitSignals {
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
		if raw_line.len() < 4 {
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

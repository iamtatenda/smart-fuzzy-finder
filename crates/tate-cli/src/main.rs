use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{ArgAction, Args, Parser, Subcommand};
use smart_fuzzy_finder_core::{grep_project, record_open, search, CacheConfig, SearchConfig, SearchRequest};

#[derive(Debug, Parser)]
#[command(name = "smart-fuzzy-finder", about = "Fast smart fuzzy finder for humans and agents")]
struct Cli {
	#[command(subcommand)]
	command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
	/// Fuzzy-find files with smart ranking
	Find(FindArgs),
	/// Search text in project files
	Grep(GrepArgs),
	/// Record that a file was opened (history signal)
	Touch(TouchArgs),
}

#[derive(Debug, Args)]
struct FindArgs {
	#[arg(long, default_value = ".")]
	root: PathBuf,
	#[arg(long)]
	query: String,
	#[arg(long, default_value_t = 60)]
	limit: usize,
	#[arg(long)]
	include_hidden: bool,
	#[arg(long = "no-cache", action = ArgAction::SetFalse, default_value_t = true)]
	use_cache: bool,
	#[arg(long, default_value_t = 30)]
	cache_ttl: u64,
	#[arg(long)]
	rebuild_cache: bool,
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct GrepArgs {
	#[arg(long, default_value = ".")]
	root: PathBuf,
	#[arg(long)]
	query: String,
	#[arg(long, default_value_t = 200)]
	limit: usize,
	#[arg(long)]
	include_hidden: bool,
	#[arg(long = "no-cache", action = ArgAction::SetFalse, default_value_t = true)]
	use_cache: bool,
	#[arg(long, default_value_t = 30)]
	cache_ttl: u64,
	#[arg(long)]
	rebuild_cache: bool,
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct TouchArgs {
	#[arg(long)]
	path: String,
}

fn main() -> Result<()> {
	let cli = Cli::parse();

	match cli.command {
		Commands::Find(args) => run_find(args),
		Commands::Grep(args) => run_grep(args),
		Commands::Touch(args) => run_touch(args),
	}
}

fn run_find(args: FindArgs) -> Result<()> {
	let root = args
		.root
		.canonicalize()
		.with_context(|| format!("failed to resolve root: {}", args.root.display()))?;

	let req = SearchRequest {
		root: root.to_string_lossy().to_string(),
		query: args.query,
		limit: args.limit,
		include_hidden: args.include_hidden,
		use_cache: args.use_cache,
		cache_ttl_secs: args.cache_ttl,
		rebuild_cache: args.rebuild_cache,
	};

	let results = search(&req, &SearchConfig::default());
	if args.json {
		println!("{}", serde_json::to_string(&results)?);
	} else {
		for item in results {
			println!("{:.4}\t{}", item.score, item.path);
		}
	}

	Ok(())
}

fn run_grep(args: GrepArgs) -> Result<()> {
	let root = args
		.root
		.canonicalize()
		.with_context(|| format!("failed to resolve root: {}", args.root.display()))?;

	let cache = CacheConfig {
		use_cache: args.use_cache,
		ttl_secs: args.cache_ttl,
		rebuild: args.rebuild_cache,
	};
	let results = grep_project(&root, &args.query, args.limit, args.include_hidden, &cache);
	if args.json {
		println!("{}", serde_json::to_string(&results)?);
	} else {
		for item in results {
			println!("{}:{}:{}:{}", item.path, item.line, item.column, item.text);
		}
	}

	Ok(())
}

fn run_touch(args: TouchArgs) -> Result<()> {
	record_open(&args.path).with_context(|| format!("failed to record usage for {}", args.path))?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use clap::Parser;

	use super::{Cli, Commands};

	#[test]
	fn parses_find_command() {
		let cli = Cli::try_parse_from([
			"smart-fuzzy-finder",
			"find",
			"--query",
			"finder",
			"--limit",
			"20",
			"--no-cache",
		])
		.expect("parse find command");

		assert!(matches!(cli.command, Commands::Find(_)));
	}

	#[test]
	fn parses_grep_command() {
		let cli = Cli::try_parse_from([
			"smart-fuzzy-finder",
			"grep",
			"--query",
			"SearchRequest",
			"--cache-ttl",
			"120",
		])
		.expect("parse grep command");

		assert!(matches!(cli.command, Commands::Grep(_)));
	}

	#[test]
	fn parses_touch_command() {
		let cli = Cli::try_parse_from([
			"smart-fuzzy-finder",
			"touch",
			"--path",
			"src/main.rs",
		])
		.expect("parse touch command");

		assert!(matches!(cli.command, Commands::Touch(_)));
	}
}

# smart-fuzzy-finder

Fast smart fuzzy finder for Neovim and AI-friendly CLI workflows.

## What It Does

- Rust core for speed (`smart-fuzzy-finder-core`)
- Standalone CLI (`smart-fuzzy-finder`) for agents and terminal workflows
- Neovim plugin with:
	- Floating finder window
	- Preview pane
	- Configurable keymaps for file finding and grep
- Smart ranking that combines:
	- Fuzzy match quality
	- Typo tolerance
	- File recency/usage history
	- Git status signals (modified/untracked boosts)
- Optional persistent on-disk file index cache for faster repeated runs

## Workspace Layout

- `crates/tate-core`: finder engine, ranking, git/history signals
- `crates/tate-cli`: binary commands (`find`, `grep`, `touch`)
- `lua/smart_fuzzy_finder`: plugin runtime (`config`, `picker`, `init`)
- `plugin/smart_fuzzy_finder.lua`: plugin entrypoint
- `lua/tate` and `plugin/tate.lua`: compatibility shims

## Build

```bash
cargo build --release
```

Run tests:

```bash
cargo test
```

Binary path:

```bash
./target/release/smart-fuzzy-finder
```

## CLI Usage

### Smart File Search

```bash
smart-fuzzy-finder find --root . --query "fndr" --limit 50
```

Disable cache (fresh index walk):

```bash
smart-fuzzy-finder find --root . --query "fndr" --no-cache
```

Tune cache behavior:

```bash
smart-fuzzy-finder find --root . --query "fndr" --cache-ttl 120
smart-fuzzy-finder find --root . --query "fndr" --rebuild-cache
```

JSON output for AI agents:

```bash
smart-fuzzy-finder find --root . --query "histry" --limit 100 --json
```

### Project Grep

```bash
smart-fuzzy-finder grep --root . --query "SearchRequest" --limit 200
```

Grep with fresh index (no cache):

```bash
smart-fuzzy-finder grep --root . --query "SearchRequest" --no-cache
```

JSON output:

```bash
smart-fuzzy-finder grep --root . --query "finder" --json
```

### Record File Usage Signal

```bash
smart-fuzzy-finder touch --path crates/tate-core/src/finder.rs
```

History is stored at:

- `$XDG_STATE_HOME/smart-fuzzy-finder/history.json`
- fallback: `~/.local/state/smart-fuzzy-finder/history.json`

Index cache is stored at:

- `$XDG_CACHE_HOME/smart-fuzzy-finder/index/*.json`
- fallback: `~/.cache/smart-fuzzy-finder/index/*.json`

## Neovim Setup

### lazy.nvim

```lua
{
	"iamtatenda/tate.nvim",
	config = function()
		require("smart_fuzzy_finder").setup({
			binary = "smart-fuzzy-finder", -- or absolute path to built binary
			limit = 80,
			include_hidden = false,
			use_cache = true,
			cache_ttl = 30,
			rebuild_cache = false,
			keymaps = {
				find_files = "<leader>ff",
				live_grep = "<leader>fg",
				grep_cword = "<leader>fw",
			},
		})
	end,
}
```

### Commands

- `:SmartFuzzyFind` opens floating file finder + preview
- `:SmartFuzzyGrep` prompts for project grep and loads quickfix
- `:SmartFuzzyGrepWord` greps word under cursor

### Keymaps

Keymaps are configurable under `keymaps` in `setup()`.

## Finder Behavior

Search score uses weighted components:

- `fuzzy`: subsequence quality + contiguous streak bonus + path boundary bonus
- `typo`: normalized Levenshtein bonus for filename similarity
- `recency`: usage count and recency decay from history store
- `git`: boost for modified and untracked files
- `extension`: tiny bonus when query extension matches file extension

## Notes

- CLI is intended to be composable with agent tooling via `--json`.
- Current `grep` is line-based and optimized for fast practical use.
- If `smart-fuzzy-finder` is not on `PATH`, set `binary` in `require("smart_fuzzy_finder").setup()`.
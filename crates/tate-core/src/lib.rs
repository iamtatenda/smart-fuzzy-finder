pub mod cache;
pub mod finder;
pub mod git;
pub mod history;
pub mod types;

pub use finder::{build_index, grep_project, record_open, search};
pub use cache::CacheConfig;
pub use types::{GrepResult, MatchResult, SearchConfig, SearchRequest};

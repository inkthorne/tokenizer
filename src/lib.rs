//! High-performance file tokenizer with bitmap-based indexing
//!
//! This library provides fast file filtering through an inverted index
//! using roaring bitmaps for efficient set operations.
//!
//! # Example
//!
//! ```no_run
//! use tokenizer::{scan_and_index, query, save_index, load_index, ScanConfig};
//! use std::path::Path;
//!
//! // Build an index
//! let config = ScanConfig::default();
//! let index = scan_and_index(Path::new("/path/to/code"), &config).unwrap();
//!
//! // Save to disk
//! save_index(&index, Path::new("index.tkix")).unwrap();
//!
//! // Load and query
//! let index = load_index(Path::new("index.tkix")).unwrap();
//! let results = query(&index, "async tokio runtime");
//!
//! for file in results.files {
//!     println!("{}", file.display());
//! }
//! ```

mod error;
mod glob;
mod index;
mod persistence;
mod query;
mod scanner;
mod tokenizer;
mod trigram;

// Re-export public API
pub use error::{Result, TokenizerError};
pub use glob::{glob_files, GlobOptions, GlobResult};
pub use index::{
    ExactTokenIndex, IndexHeader, IndexMetadata, PathIndex, TokenIndex, TrigramIndex,
    FORMAT_VERSION,
};
pub use persistence::{
    // New split index API
    exact_file, load_exact, load_exact_mmap, load_paths, load_paths_mmap, load_trigram,
    load_trigram_mmap, paths_file, save_all, save_exact, save_paths, save_trigram, trigram_file,
    validate_index_match,
    // Legacy single-file API (deprecated)
    index_exists, load_index, load_index_mmap, save_index,
};
pub use query::{query, query_exact, query_fuzzy, query_with_options, QueryOptions, QueryResult};
pub use scanner::{scan_and_build_indexes, scan_and_index, ScanConfig};
pub use tokenizer::{
    extract_exact_tokens_from_file, hash_token, tokenize, tokenize_exact, tokenize_query,
    tokenize_query_exact, MIN_TOKEN_LENGTH,
};
pub use trigram::{
    extract_query_trigrams, extract_trigrams, extract_trigrams_from_file, pack_trigram,
    unpack_trigram, MIN_TRIGRAM_TOKEN_LENGTH,
};

/// Format a number with thousand separators (e.g., 1234567 -> "1,234,567")
pub fn fmt_num(n: impl std::fmt::Display) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result
}

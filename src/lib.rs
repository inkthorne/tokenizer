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

// Re-export public API
pub use error::{Result, TokenizerError};
pub use glob::{glob_files, GlobOptions, GlobResult};
pub use index::{IndexMetadata, TokenIndex};
pub use persistence::{index_exists, load_index, load_index_mmap, save_index};
pub use query::{query, query_with_options, QueryOptions, QueryResult};
pub use scanner::{scan_and_index, ScanConfig};
pub use tokenizer::{hash_token, tokenize, tokenize_query, MIN_TOKEN_LENGTH};

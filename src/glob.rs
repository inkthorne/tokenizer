use crate::error::{Result, TokenizerError};
use crate::index::TokenIndex;
use globset::Glob;
use std::path::PathBuf;

/// Options for glob file search
#[derive(Debug, Clone, Default)]
pub struct GlobOptions {
    /// Maximum number of results to return
    pub limit: Option<usize>,
}

/// Result of a glob file search
#[derive(Debug, Clone)]
pub struct GlobResult {
    /// Matching file paths
    pub files: Vec<PathBuf>,
    /// The pattern that was searched
    pub pattern: String,
    /// Total files scanned
    pub files_scanned: usize,
}

/// Search indexed filenames using a glob pattern
///
/// Matches against filenames only (not full paths).
/// Supports standard glob patterns: `*`, `?`, `[abc]`, `[!abc]`, etc.
///
/// # Examples
/// - `*.rs` - matches all Rust files
/// - `test_*.py` - matches Python test files
/// - `*config*` - matches files containing "config"
pub fn glob_files(index: &TokenIndex, pattern: &str, options: &GlobOptions) -> Result<GlobResult> {
    let glob = Glob::new(pattern)
        .map_err(|e| TokenizerError::InvalidPattern(e.to_string()))?;
    let matcher = glob.compile_matcher();

    let files_scanned = index.file_count();
    let limit = options.limit.unwrap_or(usize::MAX);

    let files: Vec<PathBuf> = index
        .iter_filenames()
        .filter(|(_, filename)| matcher.is_match(filename))
        .take(limit)
        .map(|(file_id, _)| index.get_file_path(file_id).unwrap())
        .collect();

    Ok(GlobResult {
        files,
        pattern: pattern.to_string(),
        files_scanned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_index() -> TokenIndex {
        let mut index = TokenIndex::new(PathBuf::from("/test"));
        index.register_file(PathBuf::from("/test/src/main.rs"));
        index.register_file(PathBuf::from("/test/src/lib.rs"));
        index.register_file(PathBuf::from("/test/src/utils.rs"));
        index.register_file(PathBuf::from("/test/tests/test_main.rs"));
        index.register_file(PathBuf::from("/test/tests/test_utils.rs"));
        index.register_file(PathBuf::from("/test/config.json"));
        index.register_file(PathBuf::from("/test/README.md"));
        index.register_file(PathBuf::from("/test/Cargo.toml"));
        index
    }

    #[test]
    fn test_glob_extension() {
        let index = create_test_index();
        let options = GlobOptions::default();

        let result = glob_files(&index, "*.rs", &options).unwrap();
        assert_eq!(result.files.len(), 5);
        assert!(result.files.iter().all(|p| p.extension().unwrap() == "rs"));
    }

    #[test]
    fn test_glob_prefix() {
        let index = create_test_index();
        let options = GlobOptions::default();

        let result = glob_files(&index, "test_*", &options).unwrap();
        assert_eq!(result.files.len(), 2);
        assert!(result.files.iter().all(|p| p
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("test_")));
    }

    #[test]
    fn test_glob_contains() {
        let index = create_test_index();
        let options = GlobOptions::default();

        let result = glob_files(&index, "*config*", &options).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(
            result.files[0].file_name().unwrap().to_str().unwrap(),
            "config.json"
        );
    }

    #[test]
    fn test_glob_exact() {
        let index = create_test_index();
        let options = GlobOptions::default();

        let result = glob_files(&index, "README.md", &options).unwrap();
        assert_eq!(result.files.len(), 1);
    }

    #[test]
    fn test_glob_no_matches() {
        let index = create_test_index();
        let options = GlobOptions::default();

        let result = glob_files(&index, "*.xyz", &options).unwrap();
        assert!(result.files.is_empty());
    }

    #[test]
    fn test_glob_limit() {
        let index = create_test_index();
        let options = GlobOptions { limit: Some(2) };

        let result = glob_files(&index, "*.rs", &options).unwrap();
        assert_eq!(result.files.len(), 2);
    }

    #[test]
    fn test_glob_character_class() {
        let index = create_test_index();
        let options = GlobOptions::default();

        // Match files starting with 'l' or 'm'
        let result = glob_files(&index, "[lm]*.rs", &options).unwrap();
        assert_eq!(result.files.len(), 2); // main.rs and lib.rs
    }

    #[test]
    fn test_glob_single_char_wildcard() {
        let index = create_test_index();
        let options = GlobOptions::default();

        // Match 3-letter .rs files
        let result = glob_files(&index, "???.rs", &options).unwrap();
        assert_eq!(result.files.len(), 1); // lib.rs
    }

    #[test]
    fn test_invalid_pattern() {
        let index = create_test_index();
        let options = GlobOptions::default();

        let result = glob_files(&index, "[invalid", &options);
        assert!(result.is_err());
    }

    #[test]
    fn test_result_metadata() {
        let index = create_test_index();
        let options = GlobOptions::default();

        let result = glob_files(&index, "*.rs", &options).unwrap();
        assert_eq!(result.pattern, "*.rs");
        assert_eq!(result.files_scanned, 8);
    }
}

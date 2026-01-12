use crate::index::{ExactTokenIndex, PathIndex, TokenIndex, TrigramIndex};
use crate::tokenizer::{tokenize_query, tokenize_query_exact, tokenize_query_exact_lower};
use crate::trigram::extract_query_trigrams;
use roaring::RoaringBitmap;
use std::path::PathBuf;

/// Result of a query operation
#[derive(Debug)]
pub struct QueryResult {
    /// Matching file paths
    pub files: Vec<PathBuf>,

    /// Number of tokens in the query
    pub query_token_count: usize,

    /// Number of tokens that had matches in the index
    pub matched_token_count: usize,
}

/// Query options
#[derive(Debug, Clone, Default)]
pub struct QueryOptions {
    /// Maximum number of results to return
    pub limit: Option<usize>,

    /// Require all tokens to match (AND) vs any token (OR)
    pub match_all: bool,

    /// Filter to paths containing this substring
    pub path_contains: Option<String>,

    /// Filter by glob patterns (e.g., "*.rs", "*.h")
    pub glob_patterns: Option<Vec<String>>,

    /// Exclude files with paths containing this substring
    pub exclude: Option<String>,
}

/// Execute a query against the index (AND mode by default)
pub fn query(index: &TokenIndex, query_str: &str) -> QueryResult {
    query_with_options(
        index,
        query_str,
        &QueryOptions {
            limit: None,
            match_all: true,
            ..Default::default()
        },
    )
}

/// Execute a query with options
pub fn query_with_options(
    index: &TokenIndex,
    query_str: &str,
    options: &QueryOptions,
) -> QueryResult {
    let token_hashes = tokenize_query(query_str);
    let query_token_count = token_hashes.len();

    if token_hashes.is_empty() {
        return QueryResult {
            files: vec![],
            query_token_count: 0,
            matched_token_count: 0,
        };
    }

    // Collect bitmaps for each token
    let bitmaps: Vec<&RoaringBitmap> = token_hashes
        .iter()
        .filter_map(|hash| index.get_bitmap(*hash))
        .collect();

    let matched_token_count = bitmaps.len();

    if bitmaps.is_empty() {
        return QueryResult {
            files: vec![],
            query_token_count,
            matched_token_count: 0,
        };
    }

    let result = if options.match_all {
        // AND: Intersect all bitmaps
        intersect_bitmaps(&bitmaps)
    } else {
        // OR: Union all bitmaps
        union_bitmaps(&bitmaps)
    };

    // Resolve file IDs to paths with optional limit
    let files: Vec<PathBuf> = if let Some(limit) = options.limit {
        result
            .iter()
            .take(limit)
            .filter_map(|id| index.get_file_path(id))
            .collect()
    } else {
        result
            .iter()
            .filter_map(|id| index.get_file_path(id))
            .collect()
    };

    QueryResult {
        files,
        query_token_count,
        matched_token_count,
    }
}

// ============================================================================
// Exact Mode Query (uses ExactTokenIndex)
// ============================================================================

/// Execute an exact mode query (case-sensitive, preserves _ and -)
pub fn query_exact(
    path_index: &PathIndex,
    exact_index: &ExactTokenIndex,
    query_str: &str,
    options: &QueryOptions,
) -> QueryResult {
    let token_hashes = tokenize_query_exact(query_str);
    let query_token_count = token_hashes.len();

    if token_hashes.is_empty() {
        return QueryResult {
            files: vec![],
            query_token_count: 0,
            matched_token_count: 0,
        };
    }

    // Collect bitmaps for each token
    let bitmaps: Vec<&RoaringBitmap> = token_hashes
        .iter()
        .filter_map(|hash| exact_index.get_bitmap(*hash))
        .collect();

    let matched_token_count = bitmaps.len();

    if bitmaps.is_empty() {
        return QueryResult {
            files: vec![],
            query_token_count,
            matched_token_count: 0,
        };
    }

    let result = if options.match_all {
        intersect_bitmaps(&bitmaps)
    } else {
        union_bitmaps(&bitmaps)
    };

    let files = resolve_file_ids(path_index, &result, options);

    QueryResult {
        files,
        query_token_count,
        matched_token_count,
    }
}

/// Execute a case-insensitive exact mode query
pub fn query_exact_lower(
    path_index: &PathIndex,
    exact_lower_index: &ExactTokenIndex,
    query_str: &str,
    options: &QueryOptions,
) -> QueryResult {
    let token_hashes = tokenize_query_exact_lower(query_str);
    let query_token_count = token_hashes.len();

    if token_hashes.is_empty() {
        return QueryResult {
            files: vec![],
            query_token_count: 0,
            matched_token_count: 0,
        };
    }

    // Collect bitmaps for each token
    let bitmaps: Vec<&RoaringBitmap> = token_hashes
        .iter()
        .filter_map(|hash| exact_lower_index.get_bitmap(*hash))
        .collect();

    let matched_token_count = bitmaps.len();

    if bitmaps.is_empty() {
        return QueryResult {
            files: vec![],
            query_token_count,
            matched_token_count: 0,
        };
    }

    let result = if options.match_all {
        intersect_bitmaps(&bitmaps)
    } else {
        union_bitmaps(&bitmaps)
    };

    let files = resolve_file_ids(path_index, &result, options);

    QueryResult {
        files,
        query_token_count,
        matched_token_count,
    }
}

// ============================================================================
// Fuzzy Mode Query (uses TrigramIndex)
// ============================================================================

/// Execute a fuzzy mode query (case-insensitive trigrams)
pub fn query_fuzzy(
    path_index: &PathIndex,
    trigram_index: &TrigramIndex,
    query_str: &str,
    options: &QueryOptions,
) -> QueryResult {
    let trigrams = extract_query_trigrams(query_str);
    let query_token_count = trigrams.len();

    if trigrams.is_empty() {
        return QueryResult {
            files: vec![],
            query_token_count: 0,
            matched_token_count: 0,
        };
    }

    // Collect bitmaps for each trigram
    let bitmaps: Vec<&RoaringBitmap> = trigrams
        .iter()
        .filter_map(|trigram| trigram_index.get_bitmap(*trigram))
        .collect();

    let matched_token_count = bitmaps.len();

    if bitmaps.is_empty() {
        return QueryResult {
            files: vec![],
            query_token_count,
            matched_token_count: 0,
        };
    }

    // For fuzzy search, we typically want files that match MOST trigrams
    // but not necessarily ALL (since partial matches are useful)
    let result = if options.match_all {
        intersect_bitmaps(&bitmaps)
    } else {
        union_bitmaps(&bitmaps)
    };

    let files = resolve_file_ids(path_index, &result, options);

    QueryResult {
        files,
        query_token_count,
        matched_token_count,
    }
}

/// Resolve file IDs to paths with optional filtering
fn resolve_file_ids(
    path_index: &PathIndex,
    bitmap: &RoaringBitmap,
    options: &QueryOptions,
) -> Vec<PathBuf> {
    // Build glob matcher if patterns provided
    let glob_matcher = options.glob_patterns.as_ref().and_then(|patterns| {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in patterns {
            // Make patterns case-insensitive
            if let Ok(glob) = globset::GlobBuilder::new(pattern)
                .case_insensitive(true)
                .build()
            {
                builder.add(glob);
            }
        }
        builder.build().ok()
    });

    let iter = bitmap.iter().filter_map(|id| {
        let path = path_index.get_file_path(id)?;
        let path_str = path.to_string_lossy();
        let path_lower = path_str.to_lowercase();

        // Check path_contains filter (case-insensitive)
        if let Some(ref contains) = options.path_contains {
            if !path_lower.contains(&contains.to_lowercase()) {
                return None;
            }
        }

        // Check glob patterns
        if let Some(ref matcher) = glob_matcher {
            // Match against filename only
            if let Some(filename) = path.file_name() {
                if !matcher.is_match(filename) {
                    return None;
                }
            } else {
                return None;
            }
        }

        // Check exclude filter (case-insensitive)
        if let Some(ref exclude) = options.exclude {
            if path_lower.contains(&exclude.to_lowercase()) {
                return None;
            }
        }

        Some(path)
    });

    if let Some(limit) = options.limit {
        iter.take(limit).collect()
    } else {
        iter.collect()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Intersect bitmaps, sorting by cardinality for efficiency
fn intersect_bitmaps(bitmaps: &[&RoaringBitmap]) -> RoaringBitmap {
    if bitmaps.is_empty() {
        return RoaringBitmap::new();
    }

    // Sort by cardinality (smallest first) for early termination
    let mut sorted: Vec<_> = bitmaps.iter().collect();
    sorted.sort_by_key(|b| b.len());

    let mut result = (*sorted[0]).clone();

    for bitmap in &sorted[1..] {
        result &= **bitmap;

        // Early exit if intersection is empty
        if result.is_empty() {
            break;
        }
    }

    result
}

/// Union all bitmaps
fn union_bitmaps(bitmaps: &[&RoaringBitmap]) -> RoaringBitmap {
    let mut result = RoaringBitmap::new();

    for bitmap in bitmaps {
        result |= *bitmap;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_query() {
        let index = TokenIndex::new(PathBuf::from("/test"));
        let result = query(&index, "");

        assert!(result.files.is_empty());
        assert_eq!(result.query_token_count, 0);
    }

    #[test]
    fn test_short_tokens_filtered() {
        let index = TokenIndex::new(PathBuf::from("/test"));
        // "a" and "b" are < 2 chars, should be filtered
        let result = query(&index, "a b");

        assert_eq!(result.query_token_count, 0);
    }

    #[test]
    fn test_query_no_matches() {
        let index = TokenIndex::new(PathBuf::from("/test"));
        let result = query(&index, "nonexistent token here");

        assert!(result.files.is_empty());
        assert_eq!(result.query_token_count, 3);
        assert_eq!(result.matched_token_count, 0);
    }

    #[test]
    fn test_intersect_empty_bitmaps() {
        let bitmaps: Vec<&RoaringBitmap> = vec![];
        let result = intersect_bitmaps(&bitmaps);
        assert!(result.is_empty());
    }

    #[test]
    fn test_intersect_single_bitmap() {
        let mut b = RoaringBitmap::new();
        b.insert(1);
        b.insert(2);
        b.insert(3);

        let bitmaps = vec![&b];
        let result = intersect_bitmaps(&bitmaps);

        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_intersect_multiple_bitmaps() {
        let mut b1 = RoaringBitmap::new();
        b1.insert(1);
        b1.insert(2);
        b1.insert(3);

        let mut b2 = RoaringBitmap::new();
        b2.insert(2);
        b2.insert(3);
        b2.insert(4);

        let mut b3 = RoaringBitmap::new();
        b3.insert(3);
        b3.insert(4);
        b3.insert(5);

        let bitmaps = vec![&b1, &b2, &b3];
        let result = intersect_bitmaps(&bitmaps);

        // Only 3 is in all three
        assert_eq!(result.len(), 1);
        assert!(result.contains(3));
    }

    #[test]
    fn test_union_bitmaps() {
        let mut b1 = RoaringBitmap::new();
        b1.insert(1);
        b1.insert(2);

        let mut b2 = RoaringBitmap::new();
        b2.insert(3);
        b2.insert(4);

        let bitmaps = vec![&b1, &b2];
        let result = union_bitmaps(&bitmaps);

        assert_eq!(result.len(), 4);
    }

    // ========================================================================
    // Tests for resolve_file_ids filtering (-p, -g, -x flags)
    // ========================================================================

    fn create_test_path_index() -> PathIndex {
        use crate::index::IndexHeader;

        let header = IndexHeader::new();
        let mut path_index = PathIndex::new(header, PathBuf::from("/project"));

        // Register test files: IDs 0-5
        path_index.register_file(PathBuf::from("/project/src/main.rs"));      // 0
        path_index.register_file(PathBuf::from("/project/src/lib.rs"));       // 1
        path_index.register_file(PathBuf::from("/project/test/unit.rs"));     // 2
        path_index.register_file(PathBuf::from("/project/src/util.py"));      // 3
        path_index.register_file(PathBuf::from("/project/docs/readme.md"));   // 4
        path_index.register_file(PathBuf::from("/project/src/test_helper.h")); // 5

        path_index
    }

    #[test]
    fn test_resolve_file_ids_no_filter() {
        let path_index = create_test_path_index();
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert(0);
        bitmap.insert(1);
        bitmap.insert(2);

        let options = QueryOptions::default();
        let result = resolve_file_ids(&path_index, &bitmap, &options);

        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_resolve_file_ids_path_contains() {
        let path_index = create_test_path_index();
        let mut bitmap = RoaringBitmap::new();
        for i in 0..6 {
            bitmap.insert(i);
        }

        // Filter to paths containing "src"
        let options = QueryOptions {
            path_contains: Some("src".to_string()),
            ..Default::default()
        };
        let result = resolve_file_ids(&path_index, &bitmap, &options);

        // Should match: /project/src/main.rs, /project/src/lib.rs,
        //               /project/src/util.py, /project/src/test_helper.h
        assert_eq!(result.len(), 4);
        assert!(result.iter().all(|p| p.to_string_lossy().contains("src")));
    }

    #[test]
    fn test_resolve_file_ids_path_contains_case_insensitive() {
        let path_index = create_test_path_index();
        let mut bitmap = RoaringBitmap::new();
        for i in 0..6 {
            bitmap.insert(i);
        }

        // Filter with uppercase "SRC" should still match lowercase paths
        let options = QueryOptions {
            path_contains: Some("SRC".to_string()),
            ..Default::default()
        };
        let result = resolve_file_ids(&path_index, &bitmap, &options);

        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_resolve_file_ids_glob_patterns() {
        let path_index = create_test_path_index();
        let mut bitmap = RoaringBitmap::new();
        for i in 0..6 {
            bitmap.insert(i);
        }

        // Filter to .rs files only
        let options = QueryOptions {
            glob_patterns: Some(vec!["*.rs".to_string()]),
            ..Default::default()
        };
        let result = resolve_file_ids(&path_index, &bitmap, &options);

        // Should match: main.rs, lib.rs, unit.rs, test_helper.h? No, only .rs
        assert_eq!(result.len(), 3);
        assert!(result
            .iter()
            .all(|p| p.extension().map(|e| e == "rs").unwrap_or(false)));
    }

    #[test]
    fn test_resolve_file_ids_glob_multiple_patterns() {
        let path_index = create_test_path_index();
        let mut bitmap = RoaringBitmap::new();
        for i in 0..6 {
            bitmap.insert(i);
        }

        // Filter to .rs and .h files
        let options = QueryOptions {
            glob_patterns: Some(vec!["*.rs".to_string(), "*.h".to_string()]),
            ..Default::default()
        };
        let result = resolve_file_ids(&path_index, &bitmap, &options);

        // Should match: main.rs, lib.rs, unit.rs, test_helper.h
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_resolve_file_ids_glob_case_insensitive() {
        let path_index = create_test_path_index();
        let mut bitmap = RoaringBitmap::new();
        for i in 0..6 {
            bitmap.insert(i);
        }

        // Filter with uppercase "*.RS" should match lowercase .rs files
        let options = QueryOptions {
            glob_patterns: Some(vec!["*.RS".to_string()]),
            ..Default::default()
        };
        let result = resolve_file_ids(&path_index, &bitmap, &options);

        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_resolve_file_ids_exclude() {
        let path_index = create_test_path_index();
        let mut bitmap = RoaringBitmap::new();
        for i in 0..6 {
            bitmap.insert(i);
        }

        // Exclude paths containing "test"
        let options = QueryOptions {
            exclude: Some("test".to_string()),
            ..Default::default()
        };
        let result = resolve_file_ids(&path_index, &bitmap, &options);

        // Should exclude: /project/test/unit.rs, /project/src/test_helper.h
        assert_eq!(result.len(), 4);
        assert!(result
            .iter()
            .all(|p| !p.to_string_lossy().to_lowercase().contains("test")));
    }

    #[test]
    fn test_resolve_file_ids_exclude_case_insensitive() {
        let path_index = create_test_path_index();
        let mut bitmap = RoaringBitmap::new();
        for i in 0..6 {
            bitmap.insert(i);
        }

        // Exclude with uppercase "TEST" should match lowercase paths
        let options = QueryOptions {
            exclude: Some("TEST".to_string()),
            ..Default::default()
        };
        let result = resolve_file_ids(&path_index, &bitmap, &options);

        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_resolve_file_ids_combined_filters() {
        let path_index = create_test_path_index();
        let mut bitmap = RoaringBitmap::new();
        for i in 0..6 {
            bitmap.insert(i);
        }

        // Filter: path contains "src", glob "*.rs", exclude "test"
        let options = QueryOptions {
            path_contains: Some("src".to_string()),
            glob_patterns: Some(vec!["*.rs".to_string()]),
            exclude: Some("test".to_string()),
            ..Default::default()
        };
        let result = resolve_file_ids(&path_index, &bitmap, &options);

        // Should match: /project/src/main.rs, /project/src/lib.rs
        // Excluded: /project/src/test_helper.h (has "test"), /project/src/util.py (not .rs)
        assert_eq!(result.len(), 2);
        let paths: Vec<_> = result.iter().map(|p| p.to_string_lossy().to_string()).collect();
        assert!(paths.iter().any(|p| p.contains("main.rs")));
        assert!(paths.iter().any(|p| p.contains("lib.rs")));
    }

    #[test]
    fn test_resolve_file_ids_with_limit() {
        let path_index = create_test_path_index();
        let mut bitmap = RoaringBitmap::new();
        for i in 0..6 {
            bitmap.insert(i);
        }

        let options = QueryOptions {
            limit: Some(2),
            ..Default::default()
        };
        let result = resolve_file_ids(&path_index, &bitmap, &options);

        assert_eq!(result.len(), 2);
    }

    // ========================================================================
    // Tests for OR mode (-o flag)
    // ========================================================================

    fn create_test_exact_index_with_tokens() -> (PathIndex, ExactTokenIndex) {
        use crate::index::IndexHeader;
        use crate::tokenizer::hash_token;

        let header = IndexHeader::new();
        let mut path_index = PathIndex::new(header.clone(), PathBuf::from("/project"));

        // Register test files: IDs 0-3
        path_index.register_file(PathBuf::from("/project/file_a.rs")); // 0
        path_index.register_file(PathBuf::from("/project/file_b.rs")); // 1
        path_index.register_file(PathBuf::from("/project/file_ab.rs")); // 2 (has both tokens)
        path_index.register_file(PathBuf::from("/project/file_c.rs")); // 3 (has neither)

        let mut exact_index = ExactTokenIndex::new(header);

        // Token "alpha" is in files 0 and 2
        let hash_alpha = hash_token(b"alpha");
        let mut bitmap_alpha = RoaringBitmap::new();
        bitmap_alpha.insert(0);
        bitmap_alpha.insert(2);
        exact_index.token_map.insert(hash_alpha, bitmap_alpha);

        // Token "beta" is in files 1 and 2
        let hash_beta = hash_token(b"beta");
        let mut bitmap_beta = RoaringBitmap::new();
        bitmap_beta.insert(1);
        bitmap_beta.insert(2);
        exact_index.token_map.insert(hash_beta, bitmap_beta);

        (path_index, exact_index)
    }

    #[test]
    fn test_query_exact_and_mode() {
        let (path_index, exact_index) = create_test_exact_index_with_tokens();

        // AND mode: should find only files containing BOTH "alpha" AND "beta"
        let options = QueryOptions {
            match_all: true, // AND mode
            ..Default::default()
        };
        let result = query_exact(&path_index, &exact_index, "alpha beta", &options);

        // Only file 2 has both tokens
        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].to_string_lossy().contains("file_ab.rs"));
    }

    #[test]
    fn test_query_exact_or_mode() {
        let (path_index, exact_index) = create_test_exact_index_with_tokens();

        // OR mode: should find files containing "alpha" OR "beta"
        let options = QueryOptions {
            match_all: false, // OR mode
            ..Default::default()
        };
        let result = query_exact(&path_index, &exact_index, "alpha beta", &options);

        // Files 0, 1, and 2 have at least one token
        assert_eq!(result.files.len(), 3);
        // Verify we have the expected files (can't rely on order)
        let path_strs: Vec<_> = result
            .files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert!(path_strs.iter().any(|p| p.contains("file_a.rs")));
        assert!(path_strs.iter().any(|p| p.contains("file_b.rs")));
        assert!(path_strs.iter().any(|p| p.contains("file_ab.rs")));
    }

    #[test]
    fn test_query_exact_or_mode_single_token() {
        let (path_index, exact_index) = create_test_exact_index_with_tokens();

        // OR mode with single token should be same as AND mode
        let and_options = QueryOptions {
            match_all: true,
            ..Default::default()
        };
        let or_options = QueryOptions {
            match_all: false,
            ..Default::default()
        };

        let and_result = query_exact(&path_index, &exact_index, "alpha", &and_options);
        let or_result = query_exact(&path_index, &exact_index, "alpha", &or_options);

        // Both should find the same 2 files
        assert_eq!(and_result.files.len(), 2);
        assert_eq!(or_result.files.len(), 2);
    }
}

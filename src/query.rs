use crate::index::TokenIndex;
use crate::tokenizer::tokenize_query;
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
}

/// Execute a query against the index (AND mode by default)
pub fn query(index: &TokenIndex, query_str: &str) -> QueryResult {
    query_with_options(
        index,
        query_str,
        &QueryOptions {
            limit: None,
            match_all: true,
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
}

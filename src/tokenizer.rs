use memmap2::Mmap;
use rustc_hash::{FxHashSet, FxHasher};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// Minimum token length to include
pub const MIN_TOKEN_LENGTH: usize = 2;

/// Extract tokens from a byte slice and return their hashes
pub fn tokenize(content: &[u8]) -> impl Iterator<Item = u64> + '_ {
    TokenIterator::new(content)
}

/// Tokenize a string query
pub fn tokenize_query(query: &str) -> Vec<u64> {
    tokenize(query.as_bytes()).collect()
}

/// Hash a token using FxHash (case-sensitive)
#[inline]
pub fn hash_token(token: &[u8]) -> u64 {
    let mut hasher = FxHasher::default();
    token.hash(&mut hasher);
    hasher.finish()
}

/// Iterator that yields token hashes from content
pub struct TokenIterator<'a> {
    content: &'a [u8],
    position: usize,
}

impl<'a> TokenIterator<'a> {
    pub fn new(content: &'a [u8]) -> Self {
        Self {
            content,
            position: 0,
        }
    }

    /// Skip non-alphanumeric characters
    #[inline]
    fn skip_delimiters(&mut self) {
        while self.position < self.content.len()
            && !self.content[self.position].is_ascii_alphanumeric()
        {
            self.position += 1;
        }
    }

    /// Read next token bytes
    #[inline]
    fn read_token(&mut self) -> Option<&'a [u8]> {
        let start = self.position;

        while self.position < self.content.len()
            && self.content[self.position].is_ascii_alphanumeric()
        {
            self.position += 1;
        }

        if self.position > start {
            Some(&self.content[start..self.position])
        } else {
            None
        }
    }
}

impl<'a> Iterator for TokenIterator<'a> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.skip_delimiters();

            if self.position >= self.content.len() {
                return None;
            }

            if let Some(token) = self.read_token() {
                if token.len() >= MIN_TOKEN_LENGTH {
                    return Some(hash_token(token));
                }
            } else {
                return None;
            }
        }
    }
}

// ============================================================================
// Exact Mode Tokenizer (keeps _ and - as part of tokens)
// ============================================================================

/// Check if a byte is an exact-mode delimiter
/// Exact mode keeps: a-z, A-Z, 0-9, _, -
/// Splits on: whitespace, brackets, quotes, operators, punctuation
#[inline]
fn is_exact_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t'
            | b'\n'
            | b'\r'
            | b'('
            | b')'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'<'
            | b'>'
            | b'"'
            | b'\''
            | b'`'
            | b','
            | b';'
            | b':'
            | b'.'
            | b'+'
            | b'='
            | b'/'
            | b'\\'
            | b'@'
            | b'#'
            | b'$'
            | b'%'
            | b'^'
            | b'&'
            | b'*'
            | b'!'
            | b'?'
            | b'|'
            | b'~'
            | 0 // null byte
    )
}

/// Check if a byte is part of an exact-mode token
#[inline]
fn is_exact_token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

/// Iterator that yields exact-mode token hashes from content
pub struct ExactTokenIterator<'a> {
    content: &'a [u8],
    position: usize,
}

impl<'a> ExactTokenIterator<'a> {
    pub fn new(content: &'a [u8]) -> Self {
        Self {
            content,
            position: 0,
        }
    }

    /// Skip delimiter characters
    #[inline]
    fn skip_delimiters(&mut self) {
        while self.position < self.content.len() && is_exact_delimiter(self.content[self.position])
        {
            self.position += 1;
        }
    }

    /// Read next token bytes
    #[inline]
    fn read_token(&mut self) -> Option<&'a [u8]> {
        let start = self.position;

        while self.position < self.content.len()
            && is_exact_token_char(self.content[self.position])
        {
            self.position += 1;
        }

        if self.position > start {
            Some(&self.content[start..self.position])
        } else {
            // Skip unknown characters that aren't delimiters but aren't token chars either
            if self.position < self.content.len() {
                self.position += 1;
            }
            None
        }
    }
}

impl<'a> Iterator for ExactTokenIterator<'a> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.skip_delimiters();

            if self.position >= self.content.len() {
                return None;
            }

            if let Some(token) = self.read_token() {
                if token.len() >= MIN_TOKEN_LENGTH {
                    return Some(hash_token(token));
                }
            }
        }
    }
}

/// Extract exact-mode tokens from a byte slice
pub fn tokenize_exact(content: &[u8]) -> impl Iterator<Item = u64> + '_ {
    ExactTokenIterator::new(content)
}

/// Tokenize a string query in exact mode
pub fn tokenize_query_exact(query: &str) -> Vec<u64> {
    tokenize_exact(query.as_bytes()).collect()
}

/// Extract unique exact-mode token hashes from a file
pub fn extract_exact_tokens_from_file(path: &Path) -> std::io::Result<Vec<u64>> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;

    if metadata.len() == 0 {
        return Ok(Vec::new());
    }

    let mmap = unsafe { Mmap::map(&file)? };

    // Check for binary file (null bytes in first 8KB)
    let check_len = std::cmp::min(8192, mmap.len());
    if mmap[..check_len].contains(&0) {
        return Ok(Vec::new());
    }

    let unique_tokens: FxHashSet<u64> = tokenize_exact(&mmap[..]).collect();
    Ok(unique_tokens.into_iter().collect())
}

// ============================================================================
// Legacy tokenizer (splits on all non-alphanumeric)
// ============================================================================

/// Extract unique token hashes from a file using memory mapping
pub fn extract_tokens_from_file(path: &Path) -> std::io::Result<Vec<u64>> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;

    // Handle empty files
    if metadata.len() == 0 {
        return Ok(Vec::new());
    }

    let mmap = unsafe { Mmap::map(&file)? };

    // Check for binary file (null bytes in first 8KB)
    let check_len = std::cmp::min(8192, mmap.len());
    if mmap[..check_len].contains(&0) {
        return Ok(Vec::new()); // Skip binary files
    }

    // Use FxHashSet to deduplicate tokens within a file
    let unique_tokens: FxHashSet<u64> = tokenize(&mmap[..]).collect();

    Ok(unique_tokens.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_basic() {
        let content = b"Hello, World! This is a test.";
        let tokens: Vec<_> = tokenize(content).collect();

        // Should have: Hello, World, This, is, test (a is < 2 chars)
        assert_eq!(tokens.len(), 5);
    }

    #[test]
    fn test_case_sensitive() {
        let hash1 = hash_token(b"Hello");
        let hash2 = hash_token(b"hello");
        let hash3 = hash_token(b"HELLO");

        // All different cases should produce different hashes
        assert_ne!(hash1, hash2);
        assert_ne!(hash2, hash3);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_min_length_filter() {
        let content = b"a ab abc abcd";
        let tokens: Vec<_> = tokenize(content).collect();

        // ab, abc, and abcd should pass (>= 2 chars)
        assert_eq!(tokens.len(), 3);
    }

    #[test]
    fn test_empty_input() {
        let content = b"";
        let tokens: Vec<_> = tokenize(content).collect();
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_only_delimiters() {
        let content = b"!@#$%^&*()";
        let tokens: Vec<_> = tokenize(content).collect();
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_numeric_tokens() {
        let content = b"test123 456 ab7cd";
        let tokens: Vec<_> = tokenize(content).collect();
        // test123, 456, ab7cd (all >= 3 chars)
        assert_eq!(tokens.len(), 3);
    }

    // Exact mode tests
    #[test]
    fn test_exact_tokenize_preserves_underscores() {
        let content = b"run_game start_server my_var";
        let tokens: Vec<_> = tokenize_exact(content).collect();
        // Should have 3 tokens: run_game, start_server, my_var
        assert_eq!(tokens.len(), 3);

        // Verify the tokens are different from legacy tokenizer
        let legacy_tokens: Vec<_> = tokenize(content).collect();
        // Legacy would have 6: run, game, start, server, my, var
        assert_eq!(legacy_tokens.len(), 6);
    }

    #[test]
    fn test_exact_tokenize_preserves_hyphens() {
        let content = b"my-component user-service kebab-case";
        let tokens: Vec<_> = tokenize_exact(content).collect();
        // Should have 3 tokens
        assert_eq!(tokens.len(), 3);

        // Legacy would split on hyphens
        let legacy_tokens: Vec<_> = tokenize(content).collect();
        assert_eq!(legacy_tokens.len(), 6);
    }

    #[test]
    fn test_exact_tokenize_mixed() {
        let content = b"func(my_var, other-arg)";
        let tokens: Vec<_> = tokenize_exact(content).collect();
        // Should have 3 tokens: func, my_var, other-arg
        assert_eq!(tokens.len(), 3);

        // Verify hash of my_var matches query
        let query_tokens = tokenize_query_exact("my_var");
        assert_eq!(query_tokens.len(), 1);

        // The hash should match
        let content_hashes: std::collections::HashSet<_> = tokens.into_iter().collect();
        assert!(content_hashes.contains(&query_tokens[0]));
    }

    #[test]
    fn test_exact_tokenize_splits_on_dots() {
        let content = b"package.module.Class";
        let tokens: Vec<_> = tokenize_exact(content).collect();
        // Should split on dots: package, module, Class
        assert_eq!(tokens.len(), 3);
    }

    #[test]
    fn test_exact_tokenize_splits_on_operators() {
        let content = b"a+b=c*d/e";
        let tokens: Vec<_> = tokenize_exact(content).collect();
        // Single chars filtered out (< 2 chars), so only empty
        assert_eq!(tokens.len(), 0);
    }

    #[test]
    fn test_exact_query_matches_content() {
        let content = b"def process_data(input_buffer):";
        let content_tokens: FxHashSet<_> = tokenize_exact(content).collect();

        // Query for exact match
        let query = tokenize_query_exact("process_data");
        assert_eq!(query.len(), 1);
        assert!(content_tokens.contains(&query[0]));

        // Query for partial should NOT match
        let partial_query = tokenize_query_exact("process");
        assert!(!content_tokens.contains(&partial_query[0]));
    }
}

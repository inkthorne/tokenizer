//! Trigram extraction for fuzzy text search
//!
//! Trigrams are 3-character sequences used for approximate string matching.
//! This module provides case-insensitive trigram extraction for fuzzy search.

use memmap2::Mmap;
use rustc_hash::FxHashSet;
use std::fs::File;
use std::path::Path;

/// Minimum length for a token to generate trigrams from
/// Tokens shorter than 3 chars don't produce any trigrams
pub const MIN_TRIGRAM_TOKEN_LENGTH: usize = 3;

/// Pack 3 bytes into a u32 for efficient storage
#[inline]
pub fn pack_trigram(a: u8, b: u8, c: u8) -> u32 {
    ((a as u32) << 16) | ((b as u32) << 8) | (c as u32)
}

/// Unpack a u32 trigram into 3 bytes
#[inline]
pub fn unpack_trigram(trigram: u32) -> (u8, u8, u8) {
    (
        ((trigram >> 16) & 0xFF) as u8,
        ((trigram >> 8) & 0xFF) as u8,
        (trigram & 0xFF) as u8,
    )
}

/// Convert a byte to lowercase ASCII
#[inline]
fn to_lowercase(b: u8) -> u8 {
    if b.is_ascii_uppercase() {
        b + 32
    } else {
        b
    }
}

/// Check if a byte is a valid token character for trigram extraction
/// Includes: a-z, A-Z, 0-9, _, -
#[inline]
fn is_trigram_token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

/// Iterator that extracts trigrams from content
pub struct TrigramIterator<'a> {
    content: &'a [u8],
    position: usize,
    // Buffer to hold current token bytes (lowercase)
    token_buf: Vec<u8>,
    // Current position within token_buf for trigram extraction
    token_pos: usize,
}

impl<'a> TrigramIterator<'a> {
    pub fn new(content: &'a [u8]) -> Self {
        Self {
            content,
            position: 0,
            token_buf: Vec::with_capacity(256),
            token_pos: 0,
        }
    }

    /// Skip non-token characters
    #[inline]
    fn skip_delimiters(&mut self) {
        while self.position < self.content.len()
            && !is_trigram_token_char(self.content[self.position])
        {
            self.position += 1;
        }
    }

    /// Read next token into buffer, converting to lowercase.
    /// Skips tokens shorter than MIN_TRIGRAM_TOKEN_LENGTH.
    fn read_next_token(&mut self) -> bool {
        loop {
            self.skip_delimiters();

            if self.position >= self.content.len() {
                return false;
            }

            self.token_buf.clear();
            self.token_pos = 0;

            while self.position < self.content.len()
                && is_trigram_token_char(self.content[self.position])
            {
                self.token_buf
                    .push(to_lowercase(self.content[self.position]));
                self.position += 1;
            }

            // If token is long enough, return success
            // Otherwise loop to try next token
            if self.token_buf.len() >= MIN_TRIGRAM_TOKEN_LENGTH {
                return true;
            }
        }
    }

    /// Get next trigram from current token
    fn next_trigram_from_token(&mut self) -> Option<u32> {
        if self.token_pos + 3 <= self.token_buf.len() {
            let trigram = pack_trigram(
                self.token_buf[self.token_pos],
                self.token_buf[self.token_pos + 1],
                self.token_buf[self.token_pos + 2],
            );
            self.token_pos += 1;
            Some(trigram)
        } else {
            None
        }
    }
}

impl<'a> Iterator for TrigramIterator<'a> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Try to get next trigram from current token
            if let Some(trigram) = self.next_trigram_from_token() {
                return Some(trigram);
            }

            // Need to read next token
            if !self.read_next_token() {
                return None;
            }

            // Try again with new token
            if let Some(trigram) = self.next_trigram_from_token() {
                return Some(trigram);
            }
        }
    }
}

/// Extract trigrams from a byte slice
pub fn extract_trigrams(content: &[u8]) -> impl Iterator<Item = u32> + '_ {
    TrigramIterator::new(content)
}

/// Extract trigrams from a query string (case-insensitive)
pub fn extract_query_trigrams(query: &str) -> Vec<u32> {
    extract_trigrams(query.as_bytes()).collect()
}

/// Extract unique trigrams from a file
pub fn extract_trigrams_from_file(path: &Path) -> std::io::Result<Vec<u32>> {
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

    let unique_trigrams: FxHashSet<u32> = extract_trigrams(&mmap[..]).collect();
    Ok(unique_trigrams.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_unpack_trigram() {
        let trigram = pack_trigram(b'a', b'b', b'c');
        let (a, b, c) = unpack_trigram(trigram);
        assert_eq!(a, b'a');
        assert_eq!(b, b'b');
        assert_eq!(c, b'c');
    }

    #[test]
    fn test_extract_trigrams_simple() {
        let content = b"hello";
        let trigrams: Vec<_> = extract_trigrams(content).collect();
        // "hello" -> "hel", "ell", "llo" = 3 trigrams
        assert_eq!(trigrams.len(), 3);

        assert!(trigrams.contains(&pack_trigram(b'h', b'e', b'l')));
        assert!(trigrams.contains(&pack_trigram(b'e', b'l', b'l')));
        assert!(trigrams.contains(&pack_trigram(b'l', b'l', b'o')));
    }

    #[test]
    fn test_extract_trigrams_case_insensitive() {
        let content1 = b"Hello";
        let content2 = b"HELLO";
        let content3 = b"hello";

        let t1: Vec<_> = extract_trigrams(content1).collect();
        let t2: Vec<_> = extract_trigrams(content2).collect();
        let t3: Vec<_> = extract_trigrams(content3).collect();

        // All should produce the same trigrams (case-insensitive)
        assert_eq!(t1, t2);
        assert_eq!(t2, t3);
    }

    #[test]
    fn test_extract_trigrams_multiple_tokens() {
        let content = b"foo bar";
        let trigrams: Vec<_> = extract_trigrams(content).collect();
        // "foo" -> "foo" = 1 trigram
        // "bar" -> "bar" = 1 trigram
        assert_eq!(trigrams.len(), 2);
    }

    #[test]
    fn test_extract_trigrams_preserves_underscore() {
        let content = b"run_game";
        let trigrams: Vec<_> = extract_trigrams(content).collect();
        // "run_game" -> "run", "un_", "n_g", "_ga", "gam", "ame" = 6 trigrams
        assert_eq!(trigrams.len(), 6);

        // Verify underscore is included
        assert!(trigrams.contains(&pack_trigram(b'u', b'n', b'_')));
        assert!(trigrams.contains(&pack_trigram(b'n', b'_', b'g')));
        assert!(trigrams.contains(&pack_trigram(b'_', b'g', b'a')));
    }

    #[test]
    fn test_extract_trigrams_preserves_hyphen() {
        let content = b"my-var";
        let trigrams: Vec<_> = extract_trigrams(content).collect();
        // "my-var" -> "my-", "y-v", "-va", "var" = 4 trigrams
        assert_eq!(trigrams.len(), 4);
    }

    #[test]
    fn test_extract_trigrams_short_tokens() {
        let content = b"ab cd ef";
        let trigrams: Vec<_> = extract_trigrams(content).collect();
        // All tokens are < 3 chars, no trigrams
        assert_eq!(trigrams.len(), 0);
    }

    #[test]
    fn test_extract_trigrams_skips_short_tokens() {
        let content = b"a alfred b";
        let trigrams: Vec<_> = extract_trigrams(content).collect();
        // "alfred" -> "alf", "lfr", "fre", "red" = 4 trigrams
        // "a" and "b" should be skipped, not terminate iteration
        assert_eq!(trigrams.len(), 4);

        assert!(trigrams.contains(&pack_trigram(b'a', b'l', b'f')));
        assert!(trigrams.contains(&pack_trigram(b'l', b'f', b'r')));
        assert!(trigrams.contains(&pack_trigram(b'f', b'r', b'e')));
        assert!(trigrams.contains(&pack_trigram(b'r', b'e', b'd')));
    }

    #[test]
    fn test_extract_trigrams_mixed_short_long() {
        let content = b"a hello b world c";
        let trigrams: Vec<_> = extract_trigrams(content).collect();
        // "hello" -> "hel", "ell", "llo" = 3 trigrams
        // "world" -> "wor", "orl", "rld" = 3 trigrams
        // Total = 6 trigrams
        assert_eq!(trigrams.len(), 6);
    }

    #[test]
    fn test_query_trigrams() {
        let query_trigrams = extract_query_trigrams("UserService");

        // Should have trigrams for "userservice" (lowercase)
        // "userservice" = 11 chars -> 9 trigrams
        assert_eq!(query_trigrams.len(), 9);

        // Verify lowercase conversion
        assert!(query_trigrams.contains(&pack_trigram(b'u', b's', b'e')));
        assert!(query_trigrams.contains(&pack_trigram(b's', b'e', b'r')));
    }

    #[test]
    fn test_trigram_matching() {
        let content = b"def process_data(input):";
        let content_trigrams: FxHashSet<_> = extract_trigrams(content).collect();

        // Query for "process" should have overlapping trigrams
        let query_trigrams = extract_query_trigrams("process");

        // Check that all query trigrams are in content
        let matches = query_trigrams
            .iter()
            .filter(|t| content_trigrams.contains(t))
            .count();
        assert_eq!(matches, query_trigrams.len());

        // Query for "PROCESS" should also match (case-insensitive)
        let upper_query_trigrams = extract_query_trigrams("PROCESS");
        assert_eq!(query_trigrams, upper_query_trigrams);
    }

    #[test]
    fn test_partial_matching() {
        let content = b"getUserById";
        let content_trigrams: FxHashSet<_> = extract_trigrams(content).collect();

        // Query for "user" should have some matching trigrams
        let query_trigrams = extract_query_trigrams("user");

        let matches = query_trigrams
            .iter()
            .filter(|t| content_trigrams.contains(t))
            .count();

        // "user" trigrams: "use", "ser" = 2
        // Both should be in "getuserbyid" (lowercase)
        assert!(matches > 0);
    }

    #[test]
    fn test_empty_content() {
        let content = b"";
        let trigrams: Vec<_> = extract_trigrams(content).collect();
        assert!(trigrams.is_empty());
    }

    #[test]
    fn test_only_delimiters() {
        let content = b"!@#$%^&*()";
        let trigrams: Vec<_> = extract_trigrams(content).collect();
        assert!(trigrams.is_empty());
    }
}

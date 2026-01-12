use crate::error::{Result, TokenizerError};
use crate::index::TokenIndex;
use memmap2::Mmap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

/// Magic bytes for index file identification
const MAGIC: &[u8; 4] = b"TKIX";

/// Save index to disk
pub fn save_index(index: &TokenIndex, path: &Path) -> Result<()> {
    let file = File::create(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mut writer = BufWriter::new(file);

    // Write magic bytes
    writer
        .write_all(MAGIC)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    // Serialize with bincode 2.0 serde compat
    let config = bincode::config::standard();
    let encoded = bincode::serde::encode_to_vec(index, config)
        .map_err(|e| TokenizerError::Serialization(e.to_string()))?;

    writer
        .write_all(&encoded)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    writer
        .flush()
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    Ok(())
}

/// Load index from disk
pub fn load_index(path: &Path) -> Result<TokenIndex> {
    let file = File::open(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mut reader = BufReader::new(file);

    // Verify magic bytes
    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    if &magic != MAGIC {
        return Err(TokenizerError::InvalidIndexFormat(
            "Invalid magic bytes".to_string(),
        ));
    }

    // Read remaining bytes
    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    // Deserialize with bincode 2.0 serde compat
    let config = bincode::config::standard();
    let (mut index, _): (TokenIndex, _) = bincode::serde::decode_from_slice(&data, config)
        .map_err(|e| TokenizerError::Serialization(e.to_string()))?;

    // Validate version
    if index.metadata().version != TokenIndex::CURRENT_VERSION {
        return Err(TokenizerError::InvalidIndexFormat(format!(
            "Index version mismatch: expected {}, got {}",
            TokenIndex::CURRENT_VERSION,
            index.metadata().version
        )));
    }

    // Rebuild transient lookup table
    index.rebuild_dir_lookup();

    Ok(index)
}

/// Load index using memory mapping for faster access
pub fn load_index_mmap(path: &Path) -> Result<TokenIndex> {
    let file = File::open(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mmap = unsafe { Mmap::map(&file).map_err(|e| TokenizerError::Io(e.to_string()))? };

    // Verify magic bytes
    if mmap.len() < 4 || &mmap[..4] != MAGIC {
        return Err(TokenizerError::InvalidIndexFormat(
            "Invalid magic bytes".to_string(),
        ));
    }

    // Deserialize from memory-mapped slice
    let config = bincode::config::standard();
    let (mut index, _): (TokenIndex, _) = bincode::serde::decode_from_slice(&mmap[4..], config)
        .map_err(|e| TokenizerError::Serialization(e.to_string()))?;

    // Validate version
    if index.metadata().version != TokenIndex::CURRENT_VERSION {
        return Err(TokenizerError::InvalidIndexFormat(format!(
            "Index version mismatch: expected {}, got {}",
            TokenIndex::CURRENT_VERSION,
            index.metadata().version
        )));
    }

    // Rebuild transient lookup table
    index.rebuild_dir_lookup();

    Ok(index)
}

/// Check if an index file exists and is valid
pub fn index_exists(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }

    // Try to read magic bytes
    if let Ok(file) = File::open(path) {
        let mut reader = BufReader::new(file);
        let mut magic = [0u8; 4];
        if reader.read_exact(&mut magic).is_ok() {
            return &magic == MAGIC;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let index_path = dir.path().join("test.idx");

        let mut index = TokenIndex::new(dir.path().to_path_buf());
        index.register_file(PathBuf::from("/test/file1.txt"));
        index.add_token(12345, 0);
        index.finalize();

        save_index(&index, &index_path).unwrap();
        let loaded = load_index(&index_path).unwrap();

        assert_eq!(index.file_count(), loaded.file_count());
        assert_eq!(index.token_count(), loaded.token_count());
    }

    #[test]
    fn test_load_mmap_roundtrip() {
        let dir = tempdir().unwrap();
        let index_path = dir.path().join("test.idx");

        let mut index = TokenIndex::new(dir.path().to_path_buf());
        index.register_file(PathBuf::from("/test/file1.txt"));
        index.add_token(12345, 0);
        index.finalize();

        save_index(&index, &index_path).unwrap();
        let loaded = load_index_mmap(&index_path).unwrap();

        assert_eq!(index.file_count(), loaded.file_count());
        assert_eq!(index.token_count(), loaded.token_count());
    }

    #[test]
    fn test_index_exists() {
        let dir = tempdir().unwrap();
        let index_path = dir.path().join("test.idx");

        assert!(!index_exists(&index_path));

        let index = TokenIndex::new(dir.path().to_path_buf());
        save_index(&index, &index_path).unwrap();

        assert!(index_exists(&index_path));
    }

    #[test]
    fn test_invalid_magic() {
        let dir = tempdir().unwrap();
        let index_path = dir.path().join("bad.idx");

        std::fs::write(&index_path, b"BAAD").unwrap();

        let result = load_index(&index_path);
        assert!(result.is_err());
    }
}

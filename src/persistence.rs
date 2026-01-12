use crate::error::{Result, TokenizerError};
use crate::index::{
    ExactTokenIndex, IndexHeader, PathIndex, TokenIndex, TrigramIndex, FORMAT_VERSION,
};
use memmap2::Mmap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

// ============================================================================
// Legacy single-file index (for backward compatibility during transition)
// ============================================================================

/// Legacy magic bytes
const MAGIC_LEGACY: &[u8; 4] = b"TKIX";

/// Save legacy index to disk (DEPRECATED)
pub fn save_index(index: &TokenIndex, path: &Path) -> Result<()> {
    let file = File::create(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mut writer = BufWriter::new(file);

    writer
        .write_all(MAGIC_LEGACY)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

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

/// Load legacy index from disk (DEPRECATED)
pub fn load_index(path: &Path) -> Result<TokenIndex> {
    let file = File::open(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mut reader = BufReader::new(file);

    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    if &magic != MAGIC_LEGACY {
        return Err(TokenizerError::InvalidIndexFormat(
            "Invalid magic bytes".to_string(),
        ));
    }

    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    let config = bincode::config::standard();
    let (mut index, _): (TokenIndex, _) = bincode::serde::decode_from_slice(&data, config)
        .map_err(|e| TokenizerError::Serialization(e.to_string()))?;

    if index.metadata().version != TokenIndex::CURRENT_VERSION {
        return Err(TokenizerError::InvalidIndexFormat(format!(
            "Index version mismatch: expected {}, got {}",
            TokenIndex::CURRENT_VERSION,
            index.metadata().version
        )));
    }

    index.rebuild_dir_lookup();
    Ok(index)
}

/// Load legacy index using memory mapping (DEPRECATED)
pub fn load_index_mmap(path: &Path) -> Result<TokenIndex> {
    let file = File::open(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mmap = unsafe { Mmap::map(&file).map_err(|e| TokenizerError::Io(e.to_string()))? };

    if mmap.len() < 4 || &mmap[..4] != MAGIC_LEGACY {
        return Err(TokenizerError::InvalidIndexFormat(
            "Invalid magic bytes".to_string(),
        ));
    }

    let config = bincode::config::standard();
    let (mut index, _): (TokenIndex, _) = bincode::serde::decode_from_slice(&mmap[4..], config)
        .map_err(|e| TokenizerError::Serialization(e.to_string()))?;

    if index.metadata().version != TokenIndex::CURRENT_VERSION {
        return Err(TokenizerError::InvalidIndexFormat(format!(
            "Index version mismatch: expected {}, got {}",
            TokenIndex::CURRENT_VERSION,
            index.metadata().version
        )));
    }

    index.rebuild_dir_lookup();
    Ok(index)
}

/// Magic bytes for each file type
pub const MAGIC_PATHS: &[u8; 4] = b"TKIP";
pub const MAGIC_EXACT: &[u8; 4] = b"TKIE";
pub const MAGIC_TRIGRAM: &[u8; 4] = b"TKIT";

/// File extensions for the index files
pub const EXT_PATHS: &str = "paths";
pub const EXT_EXACT: &str = "exact";
pub const EXT_EXACT_LOWER: &str = "exacti";
pub const EXT_TRIGRAM: &str = "tri";

/// Get the paths file path from base path
pub fn paths_file(base: &Path) -> std::path::PathBuf {
    base.with_extension(EXT_PATHS)
}

/// Get the exact tokens file path from base path
pub fn exact_file(base: &Path) -> std::path::PathBuf {
    base.with_extension(EXT_EXACT)
}

/// Get the case-insensitive exact tokens file path from base path
pub fn exact_lower_file(base: &Path) -> std::path::PathBuf {
    base.with_extension(EXT_EXACT_LOWER)
}

/// Get the trigram file path from base path
pub fn trigram_file(base: &Path) -> std::path::PathBuf {
    base.with_extension(EXT_TRIGRAM)
}

// ============================================================================
// Save functions
// ============================================================================

/// Save path index to disk
pub fn save_paths(index: &PathIndex, path: &Path) -> Result<()> {
    let file = File::create(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mut writer = BufWriter::new(file);

    // Write magic bytes
    writer
        .write_all(MAGIC_PATHS)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    // Serialize with bincode
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

/// Save exact token index to disk
pub fn save_exact(index: &ExactTokenIndex, path: &Path) -> Result<()> {
    let file = File::create(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mut writer = BufWriter::new(file);

    writer
        .write_all(MAGIC_EXACT)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

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

/// Save trigram index to disk
pub fn save_trigram(index: &TrigramIndex, path: &Path) -> Result<()> {
    let file = File::create(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mut writer = BufWriter::new(file);

    writer
        .write_all(MAGIC_TRIGRAM)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

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

/// Save all three index files at once
pub fn save_all(
    paths: &PathIndex,
    exact: &ExactTokenIndex,
    exact_lower: &ExactTokenIndex,
    trigram: &TrigramIndex,
    base_path: &Path,
) -> Result<()> {
    save_paths(paths, &paths_file(base_path))?;
    save_exact(exact, &exact_file(base_path))?;
    save_exact(exact_lower, &exact_lower_file(base_path))?;
    save_trigram(trigram, &trigram_file(base_path))?;
    Ok(())
}

// ============================================================================
// Load functions
// ============================================================================

/// Load path index from disk
pub fn load_paths(path: &Path) -> Result<PathIndex> {
    let file = File::open(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mut reader = BufReader::new(file);

    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    if &magic != MAGIC_PATHS {
        return Err(TokenizerError::InvalidIndexFormat(
            "Invalid magic bytes for paths file".to_string(),
        ));
    }

    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    let config = bincode::config::standard();
    let (mut index, _): (PathIndex, _) = bincode::serde::decode_from_slice(&data, config)
        .map_err(|e| TokenizerError::Serialization(e.to_string()))?;

    if index.header.version != FORMAT_VERSION {
        return Err(TokenizerError::InvalidIndexFormat(format!(
            "Version mismatch: expected {}, got {}",
            FORMAT_VERSION, index.header.version
        )));
    }

    index.rebuild_dir_lookup();
    Ok(index)
}

/// Load path index using memory mapping
pub fn load_paths_mmap(path: &Path) -> Result<PathIndex> {
    let file = File::open(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mmap = unsafe { Mmap::map(&file).map_err(|e| TokenizerError::Io(e.to_string()))? };

    if mmap.len() < 4 || &mmap[..4] != MAGIC_PATHS {
        return Err(TokenizerError::InvalidIndexFormat(
            "Invalid magic bytes for paths file".to_string(),
        ));
    }

    let config = bincode::config::standard();
    let (mut index, _): (PathIndex, _) = bincode::serde::decode_from_slice(&mmap[4..], config)
        .map_err(|e| TokenizerError::Serialization(e.to_string()))?;

    if index.header.version != FORMAT_VERSION {
        return Err(TokenizerError::InvalidIndexFormat(format!(
            "Version mismatch: expected {}, got {}",
            FORMAT_VERSION, index.header.version
        )));
    }

    index.rebuild_dir_lookup();
    Ok(index)
}

/// Load exact token index from disk
pub fn load_exact(path: &Path) -> Result<ExactTokenIndex> {
    let file = File::open(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mut reader = BufReader::new(file);

    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    if &magic != MAGIC_EXACT {
        return Err(TokenizerError::InvalidIndexFormat(
            "Invalid magic bytes for exact tokens file".to_string(),
        ));
    }

    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    let config = bincode::config::standard();
    let (index, _): (ExactTokenIndex, _) = bincode::serde::decode_from_slice(&data, config)
        .map_err(|e| TokenizerError::Serialization(e.to_string()))?;

    if index.header.version != FORMAT_VERSION {
        return Err(TokenizerError::InvalidIndexFormat(format!(
            "Version mismatch: expected {}, got {}",
            FORMAT_VERSION, index.header.version
        )));
    }

    Ok(index)
}

/// Load exact token index using memory mapping
pub fn load_exact_mmap(path: &Path) -> Result<ExactTokenIndex> {
    let file = File::open(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mmap = unsafe { Mmap::map(&file).map_err(|e| TokenizerError::Io(e.to_string()))? };

    if mmap.len() < 4 || &mmap[..4] != MAGIC_EXACT {
        return Err(TokenizerError::InvalidIndexFormat(
            "Invalid magic bytes for exact tokens file".to_string(),
        ));
    }

    let config = bincode::config::standard();
    let (index, _): (ExactTokenIndex, _) = bincode::serde::decode_from_slice(&mmap[4..], config)
        .map_err(|e| TokenizerError::Serialization(e.to_string()))?;

    if index.header.version != FORMAT_VERSION {
        return Err(TokenizerError::InvalidIndexFormat(format!(
            "Version mismatch: expected {}, got {}",
            FORMAT_VERSION, index.header.version
        )));
    }

    Ok(index)
}

/// Load trigram index from disk
pub fn load_trigram(path: &Path) -> Result<TrigramIndex> {
    let file = File::open(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mut reader = BufReader::new(file);

    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    if &magic != MAGIC_TRIGRAM {
        return Err(TokenizerError::InvalidIndexFormat(
            "Invalid magic bytes for trigram file".to_string(),
        ));
    }

    let mut data = Vec::new();
    reader
        .read_to_end(&mut data)
        .map_err(|e| TokenizerError::Io(e.to_string()))?;

    let config = bincode::config::standard();
    let (index, _): (TrigramIndex, _) = bincode::serde::decode_from_slice(&data, config)
        .map_err(|e| TokenizerError::Serialization(e.to_string()))?;

    if index.header.version != FORMAT_VERSION {
        return Err(TokenizerError::InvalidIndexFormat(format!(
            "Version mismatch: expected {}, got {}",
            FORMAT_VERSION, index.header.version
        )));
    }

    Ok(index)
}

/// Load trigram index using memory mapping
pub fn load_trigram_mmap(path: &Path) -> Result<TrigramIndex> {
    let file = File::open(path).map_err(|e| TokenizerError::Io(e.to_string()))?;
    let mmap = unsafe { Mmap::map(&file).map_err(|e| TokenizerError::Io(e.to_string()))? };

    if mmap.len() < 4 || &mmap[..4] != MAGIC_TRIGRAM {
        return Err(TokenizerError::InvalidIndexFormat(
            "Invalid magic bytes for trigram file".to_string(),
        ));
    }

    let config = bincode::config::standard();
    let (index, _): (TrigramIndex, _) = bincode::serde::decode_from_slice(&mmap[4..], config)
        .map_err(|e| TokenizerError::Serialization(e.to_string()))?;

    if index.header.version != FORMAT_VERSION {
        return Err(TokenizerError::InvalidIndexFormat(format!(
            "Version mismatch: expected {}, got {}",
            FORMAT_VERSION, index.header.version
        )));
    }

    Ok(index)
}

/// Validate that two index files have matching index IDs
pub fn validate_index_match(header1: &IndexHeader, header2: &IndexHeader) -> Result<()> {
    if header1.index_id != header2.index_id {
        return Err(TokenizerError::IndexMismatch(
            "Index files were created from different index runs".to_string(),
        ));
    }
    Ok(())
}

/// Check if index files exist
/// Supports both legacy single-file format and new split format
pub fn index_exists(base_path: &Path) -> bool {
    // First check for legacy single-file format
    if base_path.exists() {
        if let Ok(file) = File::open(base_path) {
            let mut reader = BufReader::new(file);
            let mut magic = [0u8; 4];
            if reader.read_exact(&mut magic).is_ok() && &magic == MAGIC_LEGACY {
                return true;
            }
        }
    }

    // Then check for new split format
    let paths_path = paths_file(base_path);
    if paths_path.exists() {
        if let Ok(file) = File::open(&paths_path) {
            let mut reader = BufReader::new(file);
            let mut magic = [0u8; 4];
            if reader.read_exact(&mut magic).is_ok() && &magic == MAGIC_PATHS {
                return true;
            }
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
    fn test_header_new() {
        let header = IndexHeader::new();
        assert_eq!(header.version, FORMAT_VERSION);
        assert!(header.created_at > 0);
    }

    #[test]
    fn test_index_id_uniqueness() {
        // Generate two headers and verify they have different IDs
        let header1 = IndexHeader::new();
        let header2 = IndexHeader::new();
        assert_ne!(header1.index_id, header2.index_id);
    }

    #[test]
    fn test_paths_roundtrip() {
        let dir = tempdir().unwrap();
        let paths_path = dir.path().join("test.paths");

        let mut index = PathIndex::new(IndexHeader::new(), dir.path().to_path_buf());
        index.register_file(PathBuf::from("/test/file1.txt"));
        index.register_file(PathBuf::from("/test/file2.txt"));

        save_paths(&index, &paths_path).unwrap();
        let loaded = load_paths(&paths_path).unwrap();

        assert_eq!(index.file_count(), loaded.file_count());
        assert_eq!(index.header.index_id, loaded.header.index_id);
    }

    #[test]
    fn test_exact_roundtrip() {
        let dir = tempdir().unwrap();
        let exact_path = dir.path().join("test.exact");

        let mut index = ExactTokenIndex::new(IndexHeader::new());
        index.add_token(12345, 0);
        index.add_token(67890, 1);

        save_exact(&index, &exact_path).unwrap();
        let loaded = load_exact(&exact_path).unwrap();

        assert_eq!(index.token_count(), loaded.token_count());
    }

    #[test]
    fn test_trigram_roundtrip() {
        let dir = tempdir().unwrap();
        let tri_path = dir.path().join("test.tri");

        let mut index = TrigramIndex::new(IndexHeader::new());
        index.add_trigram(0x00616263, 0); // "abc"
        index.add_trigram(0x00626364, 1); // "bcd"

        save_trigram(&index, &tri_path).unwrap();
        let loaded = load_trigram(&tri_path).unwrap();

        assert_eq!(index.trigram_count(), loaded.trigram_count());
    }

    #[test]
    fn test_validate_index_match() {
        let header1 = IndexHeader::new();
        let header2 = IndexHeader {
            index_id: header1.index_id,
            ..IndexHeader::new()
        };
        let header3 = IndexHeader::new();

        assert!(validate_index_match(&header1, &header2).is_ok());
        assert!(validate_index_match(&header1, &header3).is_err());
    }

    #[test]
    fn test_file_path_helpers() {
        let base = Path::new("/tmp/myindex.tkix");
        assert_eq!(paths_file(base), Path::new("/tmp/myindex.paths"));
        assert_eq!(exact_file(base), Path::new("/tmp/myindex.exact"));
        assert_eq!(trigram_file(base), Path::new("/tmp/myindex.tri"));
    }
}

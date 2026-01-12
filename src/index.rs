use roaring::RoaringBitmap;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The main index structure mapping token hashes to file bitmaps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenIndex {
    /// Maps token hash (u64) to bitmap of file IDs containing that token
    pub(crate) token_map: FxHashMap<u64, RoaringBitmap>,

    /// Unique directory paths (deduplicated)
    pub(crate) directories: Vec<PathBuf>,

    /// Files as (directory_id, filename) pairs
    pub(crate) files: Vec<(u32, String)>,

    /// Transient lookup for directory deduplication during indexing
    #[serde(skip)]
    dir_lookup: FxHashMap<PathBuf, u32>,

    /// Index metadata for validation
    metadata: IndexMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMetadata {
    /// Version for format compatibility
    pub version: u32,

    /// Timestamp when index was created (unix seconds)
    pub created_at: u64,

    /// Root directory that was indexed
    pub root_path: PathBuf,

    /// Total number of files indexed
    pub file_count: u32,

    /// Total number of unique tokens
    pub token_count: u64,
}

impl TokenIndex {
    pub const CURRENT_VERSION: u32 = 2;

    /// Create a new empty index
    pub fn new(root_path: PathBuf) -> Self {
        Self {
            token_map: FxHashMap::default(),
            directories: Vec::new(),
            files: Vec::new(),
            dir_lookup: FxHashMap::default(),
            metadata: IndexMetadata {
                version: Self::CURRENT_VERSION,
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                root_path,
                file_count: 0,
                token_count: 0,
            },
        }
    }

    /// Register a file and return its ID
    pub fn register_file(&mut self, path: PathBuf) -> u32 {
        let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let dir_id = *self.dir_lookup.entry(dir.clone()).or_insert_with(|| {
            let id = self.directories.len() as u32;
            self.directories.push(dir);
            id
        });

        let file_id = self.files.len() as u32;
        self.files.push((dir_id, filename));
        self.metadata.file_count = file_id + 1;
        file_id
    }

    /// Add a token hash for a file
    pub fn add_token(&mut self, token_hash: u64, file_id: u32) {
        self.token_map
            .entry(token_hash)
            .or_insert_with(RoaringBitmap::new)
            .insert(file_id);
    }

    /// Get bitmap for a token hash
    pub fn get_bitmap(&self, token_hash: u64) -> Option<&RoaringBitmap> {
        self.token_map.get(&token_hash)
    }

    /// Get file path by ID (reconstructs from directory + filename)
    pub fn get_file_path(&self, file_id: u32) -> Option<PathBuf> {
        let (dir_id, filename) = self.files.get(file_id as usize)?;
        let dir = self.directories.get(*dir_id as usize)?;
        Some(dir.join(filename))
    }

    /// Rebuild the directory lookup table (call after deserialization)
    pub fn rebuild_dir_lookup(&mut self) {
        self.dir_lookup = self
            .directories
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), i as u32))
            .collect();
    }

    /// Finalize the index after building (compute stats)
    pub fn finalize(&mut self) {
        self.metadata.file_count = self.files.len() as u32;
        self.metadata.token_count = self.token_map.len() as u64;
    }

    /// Get metadata reference
    pub fn metadata(&self) -> &IndexMetadata {
        &self.metadata
    }

    /// Get total unique tokens
    pub fn token_count(&self) -> usize {
        self.token_map.len()
    }

    /// Get total files
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Get total unique directories
    pub fn directory_count(&self) -> usize {
        self.directories.len()
    }

    /// Iterate over all files, yielding (file_id, full_path) pairs
    pub fn iter_files(&self) -> impl Iterator<Item = (u32, PathBuf)> + '_ {
        self.files.iter().enumerate().map(|(idx, (dir_id, filename))| {
            let dir = &self.directories[*dir_id as usize];
            (idx as u32, dir.join(filename))
        })
    }

    /// Iterate over all filenames only (without directory path)
    pub fn iter_filenames(&self) -> impl Iterator<Item = (u32, &str)> + '_ {
        self.files
            .iter()
            .enumerate()
            .map(|(idx, (_, filename))| (idx as u32, filename.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_index() {
        let index = TokenIndex::new(PathBuf::from("/test"));
        assert_eq!(index.file_count(), 0);
        assert_eq!(index.token_count(), 0);
        assert_eq!(index.metadata().version, TokenIndex::CURRENT_VERSION);
    }

    #[test]
    fn test_register_file() {
        let mut index = TokenIndex::new(PathBuf::from("/test"));
        let id1 = index.register_file(PathBuf::from("/test/file1.txt"));
        let id2 = index.register_file(PathBuf::from("/test/file2.txt"));

        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(index.file_count(), 2);
    }

    #[test]
    fn test_add_and_get_token() {
        let mut index = TokenIndex::new(PathBuf::from("/test"));
        let file_id = index.register_file(PathBuf::from("/test/file.txt"));

        index.add_token(12345, file_id);

        let bitmap = index.get_bitmap(12345).unwrap();
        assert!(bitmap.contains(file_id));
        assert!(!bitmap.contains(999));
    }

    #[test]
    fn test_get_file_path() {
        let mut index = TokenIndex::new(PathBuf::from("/test"));
        let path = PathBuf::from("/test/file.txt");
        let id = index.register_file(path.clone());

        assert_eq!(index.get_file_path(id), Some(path));
        assert_eq!(index.get_file_path(999), None);
    }

    #[test]
    fn test_finalize() {
        let mut index = TokenIndex::new(PathBuf::from("/test"));
        let file_id = index.register_file(PathBuf::from("/test/file.txt"));
        index.add_token(111, file_id);
        index.add_token(222, file_id);
        index.add_token(333, file_id);

        index.finalize();

        assert_eq!(index.metadata().token_count, 3);
    }

    #[test]
    fn test_directory_deduplication() {
        let mut index = TokenIndex::new(PathBuf::from("/test"));

        // Register multiple files in the same directory
        index.register_file(PathBuf::from("/test/src/file1.txt"));
        index.register_file(PathBuf::from("/test/src/file2.txt"));
        index.register_file(PathBuf::from("/test/src/file3.txt"));
        index.register_file(PathBuf::from("/test/other/file4.txt"));

        // Should have 4 files but only 2 unique directories
        assert_eq!(index.file_count(), 4);
        assert_eq!(index.directory_count(), 2);

        // Verify paths reconstruct correctly
        assert_eq!(
            index.get_file_path(0),
            Some(PathBuf::from("/test/src/file1.txt"))
        );
        assert_eq!(
            index.get_file_path(3),
            Some(PathBuf::from("/test/other/file4.txt"))
        );
    }
}

use crate::error::{Result, TokenizerError};
use crate::index::{ExactTokenIndex, IndexHeader, PathIndex, TokenIndex, TrigramIndex};
use crate::tokenizer::{extract_exact_tokens_from_file, extract_tokens_from_file};
use crate::trigram::extract_trigrams_from_file;
use rayon::prelude::*;
use roaring::RoaringBitmap;
use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use jwalk::WalkDir as JWalkDir;
use walkdir::WalkDir;

/// Result from processing a single file in the streaming pipeline
struct FileProcessingResult {
    file_id: u32,
    exact_tokens: Vec<u64>,
    trigrams: Vec<u32>,
}

/// Configuration for scanning
#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// File extensions to include (empty = all files)
    pub extensions: Vec<String>,

    /// Patterns to exclude
    pub exclude_patterns: Vec<String>,

    /// Maximum file size to index (in bytes)
    pub max_file_size: u64,

    /// Number of files per batch for parallel processing
    pub batch_size: usize,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            extensions: vec![],
            exclude_patterns: vec![
                ".git".to_string(),
                "node_modules".to_string(),
                "target".to_string(),
                ".cache".to_string(),
                "__pycache__".to_string(),
            ],
            max_file_size: 10 * 1024 * 1024, // 10 MB
            batch_size: 1000,
        }
    }
}

/// Scan a directory and build an index (legacy, single-file format)
pub fn scan_and_index(root: &Path, config: &ScanConfig) -> Result<TokenIndex> {
    // Phase 1: Collect all file paths
    let files = collect_files(root, config)?;

    if files.is_empty() {
        return Ok(TokenIndex::new(root.to_path_buf()));
    }

    // Phase 2: Process files in parallel batches
    let index = build_index_parallel(root, files, config.batch_size)?;

    Ok(index)
}

/// Walk directory and send discovered files through a channel (runs in dedicated thread)
/// Uses jwalk for parallel directory traversal
fn walk_and_send(
    root: PathBuf,
    config: ScanConfig,
    tx: mpsc::SyncSender<PathBuf>,
) -> Result<()> {
    let exclude_patterns = config.exclude_patterns.clone();
    let extensions = config.extensions.clone();
    let max_file_size = config.max_file_size;

    for entry in JWalkDir::new(&root)
        .skip_hidden(false)
        .follow_links(false)
        .process_read_dir(move |_depth, _path, _state, children| {
            // Filter out excluded directories in parallel (runs on rayon threads)
            children.retain(|entry_result| {
                if let Ok(entry) = entry_result {
                    // Check if this is a directory we should exclude
                    if let Some(file_name) = entry.file_name.to_str() {
                        if exclude_patterns.iter().any(|p| file_name == p) {
                            return false;
                        }
                    }
                }
                true
            });
        })
    {
        let entry = entry.map_err(|e| TokenizerError::WalkDir(e.to_string()))?;

        // Skip directories - only process files
        if entry.file_type().is_dir() {
            continue;
        }

        let path = entry.path();

        // Check extension filter
        if !extensions.is_empty() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if !extensions.iter().any(|e| e == ext) {
                    continue;
                }
            } else {
                continue;
            }
        }

        // Check file size (metadata already fetched by jwalk)
        if let Ok(metadata) = entry.metadata() {
            if metadata.len() > max_file_size {
                continue;
            }
        }

        // Send to coordinator (blocks if channel full = backpressure)
        if tx.send(path.to_path_buf()).is_err() {
            // Receiver dropped, stop walking
            break;
        }
    }

    Ok(())
}

/// Process a single file and extract tokens + trigrams
fn process_single_file(file_id: u32, path: &Path) -> FileProcessingResult {
    let exact_tokens = extract_exact_tokens_from_file(path).unwrap_or_default();
    let trigrams = extract_trigrams_from_file(path).unwrap_or_default();

    FileProcessingResult {
        file_id,
        exact_tokens,
        trigrams,
    }
}

/// Merge all streaming results into final indexes
fn merge_results(
    rx: mpsc::Receiver<FileProcessingResult>,
    header: IndexHeader,
) -> (ExactTokenIndex, TrigramIndex) {
    let mut exact_map: FxHashMap<u64, RoaringBitmap> = FxHashMap::default();
    let mut trigram_map: FxHashMap<u32, RoaringBitmap> = FxHashMap::default();

    for result in rx {
        for token_hash in result.exact_tokens {
            exact_map
                .entry(token_hash)
                .or_insert_with(RoaringBitmap::new)
                .insert(result.file_id);
        }

        for trigram in result.trigrams {
            trigram_map
                .entry(trigram)
                .or_insert_with(RoaringBitmap::new)
                .insert(result.file_id);
        }
    }

    let mut exact_index = ExactTokenIndex::new(header.clone());
    exact_index.token_map = exact_map;

    let mut trigram_index = TrigramIndex::new(header);
    trigram_index.trigram_map = trigram_map;

    (exact_index, trigram_index)
}

/// Scan a directory and build all three index types (paths, exact tokens, trigrams)
///
/// Uses a streaming pipeline that processes files as they're discovered,
/// rather than collecting all files first. This provides better performance
/// on large directories by overlapping discovery with processing.
pub fn scan_and_build_indexes(
    root: &Path,
    config: &ScanConfig,
) -> Result<(PathIndex, ExactTokenIndex, TrigramIndex)> {
    // Create shared header with same index_id for all three files
    let header = IndexHeader::new();

    // Channel for discovered files (bounded for backpressure)
    let (path_tx, path_rx) = mpsc::sync_channel::<PathBuf>(1024);

    // Channel for processing results
    let (result_tx, result_rx) = mpsc::channel::<FileProcessingResult>();

    // Clone config and root for the walker thread
    let walker_config = config.clone();
    let walker_root = root.to_path_buf();

    // Spawn walker thread - discovers files and sends through channel
    let walker_handle = thread::spawn(move || walk_and_send(walker_root, walker_config, path_tx));

    // Main thread: receive paths, assign IDs, dispatch to rayon workers
    let mut path_index = PathIndex::new(header.clone(), root.to_path_buf());

    // Use rayon scope to spawn parallel workers
    rayon::scope(|s| {
        for path in path_rx {
            // Sequential: register file and get canonical ID
            let file_id = path_index.register_file(path.clone());

            // Clone sender for this task
            let tx = result_tx.clone();

            // Spawn parallel work - processing starts immediately
            s.spawn(move |_| {
                let result = process_single_file(file_id, &path);
                let _ = tx.send(result); // Ignore send errors if receiver dropped
            });
        }
    });

    // Drop the original sender so merge_results knows when all work is done
    drop(result_tx);

    // Wait for walker thread to complete and propagate any errors
    walker_handle
        .join()
        .map_err(|_| TokenizerError::WalkDir("Walker thread panicked".to_string()))??;

    // Collect and merge all results into final indexes
    let (exact_index, trigram_index) = merge_results(result_rx, header);

    Ok((path_index, exact_index, trigram_index))
}

/// Collect all files matching the configuration
fn collect_files(root: &Path, config: &ScanConfig) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_exclude(e.path(), &config.exclude_patterns))
    {
        let entry = entry.map_err(|e| TokenizerError::WalkDir(e.to_string()))?;

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();

        // Check extension filter
        if !config.extensions.is_empty() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if !config.extensions.iter().any(|e| e == ext) {
                    continue;
                }
            } else {
                continue;
            }
        }

        // Check file size
        if let Ok(metadata) = entry.metadata() {
            if metadata.len() > config.max_file_size {
                continue;
            }
        }

        files.push(path.to_path_buf());
    }

    Ok(files)
}

/// Check if a path should be excluded
fn should_exclude(path: &Path, patterns: &[String]) -> bool {
    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            if let Some(name_str) = name.to_str() {
                if patterns.iter().any(|p| name_str == p) {
                    return true;
                }
            }
        }
    }
    false
}

/// Build index using parallel processing
fn build_index_parallel(
    root: &Path,
    files: Vec<PathBuf>,
    batch_size: usize,
) -> Result<TokenIndex> {
    // Process files in parallel and collect token -> file_id mappings
    let token_maps: Vec<FxHashMap<u64, Vec<u32>>> = files
        .par_chunks(batch_size)
        .enumerate()
        .map(|(chunk_idx, chunk)| {
            let base_id = (chunk_idx * batch_size) as u32;
            let mut local_map: FxHashMap<u64, Vec<u32>> = FxHashMap::default();

            for (i, path) in chunk.iter().enumerate() {
                let file_id = base_id + i as u32;

                if let Ok(tokens) = extract_tokens_from_file(path) {
                    for token_hash in tokens {
                        local_map
                            .entry(token_hash)
                            .or_insert_with(Vec::new)
                            .push(file_id);
                    }
                }
            }

            local_map
        })
        .collect();

    // Merge all token maps
    let mut merged: FxHashMap<u64, RoaringBitmap> = FxHashMap::default();

    for local_map in token_maps {
        for (token_hash, file_ids) in local_map {
            let bitmap = merged
                .entry(token_hash)
                .or_insert_with(RoaringBitmap::new);
            for file_id in file_ids {
                bitmap.insert(file_id);
            }
        }
    }

    // Build final index with deduplicated directory paths
    let mut index = TokenIndex::new(root.to_path_buf());

    // Register all files (this deduplicates directories)
    for path in files {
        index.register_file(path);
    }

    // Assign the pre-computed token map
    index.token_map = merged;
    index.finalize();

    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{query_exact, query_fuzzy, QueryOptions};
    use std::collections::HashSet;
    use tempfile::TempDir;

    #[test]
    fn test_exact_and_fuzzy_file_ids_match() {
        // Create temp directory with test files
        let temp_dir = TempDir::new().unwrap();

        // Create a file containing "alfred" token
        let file1 = temp_dir.path().join("file1.txt");
        std::fs::write(&file1, "hello alfred world").unwrap();

        // Create another file with different content
        let file2 = temp_dir.path().join("file2.txt");
        std::fs::write(&file2, "foo bar baz").unwrap();

        // Create a third file also containing "alfred"
        let file3 = temp_dir.path().join("file3.txt");
        std::fs::write(&file3, "alfred was here").unwrap();

        // Build all three indexes
        let config = ScanConfig::default();
        let (path_index, exact_index, trigram_index) =
            scan_and_build_indexes(temp_dir.path(), &config).unwrap();

        let options = QueryOptions {
            limit: None,
            match_all: true,
        };

        // Query for "alfred" using both modes
        let exact_result = query_exact(&path_index, &exact_index, "alfred", &options);
        let fuzzy_result = query_fuzzy(&path_index, &trigram_index, "alfred", &options);

        // Both should find files
        assert!(!exact_result.files.is_empty(), "Exact should find files");
        assert!(!fuzzy_result.files.is_empty(), "Fuzzy should find files");

        // Exact results should be a subset of fuzzy results
        // (fuzzy may match more due to case-insensitivity and substring matching)
        let exact_set: HashSet<_> = exact_result.files.iter().collect();
        let fuzzy_set: HashSet<_> = fuzzy_result.files.iter().collect();

        for exact_file in &exact_set {
            assert!(
                fuzzy_set.contains(exact_file),
                "Exact match {:?} not found in fuzzy results. Exact: {:?}, Fuzzy: {:?}",
                exact_file,
                exact_set,
                fuzzy_set
            );
        }

        // Verify we found the expected files (file1 and file3 contain "alfred")
        assert_eq!(exact_result.files.len(), 2, "Should find exactly 2 files with 'alfred'");
    }

    #[test]
    fn test_fuzzy_matches_partial_token() {
        // Create temp directory with test files
        let temp_dir = TempDir::new().unwrap();

        // Create a file containing "alfred" token
        let file1 = temp_dir.path().join("file1.txt");
        std::fs::write(&file1, "hello alfred world").unwrap();

        // Build all three indexes
        let config = ScanConfig::default();
        let (path_index, exact_index, trigram_index) =
            scan_and_build_indexes(temp_dir.path(), &config).unwrap();

        let options = QueryOptions {
            limit: None,
            match_all: true,
        };

        // Exact search for "alfred" should find the file
        let exact_result = query_exact(&path_index, &exact_index, "alfred", &options);
        assert_eq!(exact_result.files.len(), 1, "Exact 'alfred' should find 1 file");

        // Fuzzy search for "lfred" (partial) should also find the file
        // because "alfred" contains trigrams: alf, lfr, fre, red
        // and "lfred" contains trigrams: lfr, fre, red
        let fuzzy_result = query_fuzzy(&path_index, &trigram_index, "lfred", &options);
        assert_eq!(fuzzy_result.files.len(), 1, "Fuzzy 'lfred' should find 1 file");

        // Both should find the same file
        assert_eq!(exact_result.files[0], fuzzy_result.files[0]);
    }

    #[test]
    fn test_should_exclude() {
        let patterns = vec![".git".to_string(), "node_modules".to_string()];

        assert!(should_exclude(
            Path::new("/project/.git/config"),
            &patterns
        ));
        assert!(should_exclude(
            Path::new("/project/node_modules/pkg"),
            &patterns
        ));
        assert!(!should_exclude(Path::new("/project/src/main.rs"), &patterns));
    }

    #[test]
    fn test_default_config() {
        let config = ScanConfig::default();
        assert!(config.extensions.is_empty());
        assert!(!config.exclude_patterns.is_empty());
        assert!(config.exclude_patterns.contains(&".git".to_string()));
    }
}

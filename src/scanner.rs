use crate::error::{Result, TokenizerError};
use crate::index::TokenIndex;
use crate::tokenizer::extract_tokens_from_file;
use rayon::prelude::*;
use roaring::RoaringBitmap;
use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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

/// Scan a directory and build an index
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

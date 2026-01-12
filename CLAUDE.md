# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build              # Debug build
cargo build --release    # Optimized release build (uses LTO)
cargo test               # Run all tests
cargo test <test_name>   # Run a single test
```

## CLI Usage

```bash
# Index current directory (all defaults)
tokenizer index

# Index with options
tokenizer index --dir ./src --extensions rs,py,js --output my.tkix

# Query (AND mode - all tokens must match)
tokenizer query "async tokio runtime"

# Query (OR mode - any token matches)
tokenizer query "async tokio" --any

# Show index statistics
tokenizer stats
```

## Architecture

This is a high-performance file search tokenizer using an inverted index with roaring bitmaps.

**Data Flow:**
```
Indexing: files → mmap → tokenize → hash (FxHash) → RoaringBitmap per token → bincode serialize
Query:    query string → tokenize → fetch bitmaps → intersect (AND) or union (OR) → file paths
```

**Key Modules:**
- `tokenizer.rs` - Zero-copy token extraction from memory-mapped files, binary file detection (null byte check in first 8KB)
- `index.rs` - `TokenIndex` struct with `FxHashMap<u64, RoaringBitmap>` mapping token hashes to file ID sets
- `scanner.rs` - Parallel file scanning with rayon, directory traversal with walkdir
- `query.rs` - Bitmap intersection/union with cardinality-based ordering for early termination
- `persistence.rs` - bincode serialization with magic bytes (`TKIX`) and version validation

**Performance Design:**
- Memory-mapped file reads (memmap2) - OS handles paging
- FxHash for token hashing (~6x faster than SipHash)
- Roaring bitmaps for compressed set operations
- Parallel indexing with rayon
- Bitmap intersection sorted by cardinality (smallest first)

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

# Query (exact match, AND mode - default)
tokenizer q "async tokio runtime"

# Query with options
tokenizer q Mannequin -f              # fuzzy match (trigrams)
tokenizer q "bob dog" -o              # OR mode (match either token)
tokenizer q Mannequin -p src          # filter paths containing "src"
tokenizer q Mannequin -g "*.rs,*.h"   # filter by glob patterns
tokenizer q Mannequin -x test         # exclude paths containing "test"
tokenizer q Mannequin -p src -x test -l 10  # combined

# Show index statistics
tokenizer stats
```

**Query flags:**
| Short | Long | Description |
|-------|------|-------------|
| `-f` | `--fuzzy` | Fuzzy match (trigrams, case-insensitive) |
| `-o` | `--or` | OR mode (match any token) |
| `-p` | `--path` | Filter paths containing substring |
| `-g` | `--glob` | Filter by glob patterns (comma-separated) |
| `-x` | `--exclude` | Exclude paths containing substring |
| `-l` | `--limit` | Max results to return |
| `-i` | `--index` | Index file path (default: index.tkix) |

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

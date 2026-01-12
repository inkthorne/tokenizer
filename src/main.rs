use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Instant;
use tokenizer::{
    exact_file, fmt_num, glob_files, index_exists, load_exact, load_exact_mmap, load_index,
    load_index_mmap, load_paths, load_paths_mmap, load_trigram, load_trigram_mmap, paths_file,
    query_exact, query_fuzzy, query_with_options, save_all, save_index, scan_and_build_indexes,
    scan_and_index, trigram_file, validate_index_match, GlobOptions, QueryOptions, ScanConfig,
    TokenizerError,
};

#[derive(Parser)]
#[command(name = "tokenizer")]
#[command(about = "High-performance file tokenizer with bitmap indexing")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build an index from a directory
    Index {
        /// Directory to index (defaults to current directory)
        #[arg(short, long, default_value = ".")]
        dir: PathBuf,

        /// Output index file path (base name for .paths, .exact, .tri files)
        #[arg(short, long, default_value = "index.tkix")]
        output: PathBuf,

        /// File extensions to include (e.g., rs,py,js)
        #[arg(short, long, value_delimiter = ',')]
        extensions: Option<Vec<String>>,

        /// Patterns to exclude
        #[arg(short = 'x', long, value_delimiter = ',')]
        exclude: Option<Vec<String>>,

        /// Maximum file size in MB
        #[arg(long, default_value = "10")]
        max_size: u64,

        /// Use legacy single-file format (deprecated)
        #[arg(long)]
        legacy: bool,
    },

    /// Query an existing index
    #[command(visible_alias = "q", after_help = "\
Examples:
  tokenizer q Mannequin                      # exact match (default)
  tokenizer q Mannequin -f                   # fuzzy match
  tokenizer q \"bob dog\" -o                   # OR mode (either token)
  tokenizer q Mannequin -p src               # paths containing \"src\"
  tokenizer q Mannequin -g \"*.rs,*.h\"        # filter by glob
  tokenizer q Mannequin -x test              # exclude \"test\"
  tokenizer q Mannequin -p src -x test -l 10 # combined")]
    Query {
        /// Search query
        query: String,

        /// Use fuzzy matching (trigrams, case-insensitive). Default is exact match.
        #[arg(short = 'f', long)]
        fuzzy: bool,

        /// Filter to paths containing substring (e.g., "src", "FortniteGame")
        #[arg(short = 'p', long)]
        path: Option<String>,

        /// Filter by glob patterns (comma-separated, e.g., "*.rs,*.h")
        #[arg(short = 'g', long, value_delimiter = ',')]
        glob: Option<Vec<String>>,

        /// Exclude files matching pattern
        #[arg(short = 'x', long)]
        exclude: Option<String>,

        /// Maximum results to return
        #[arg(short, long)]
        limit: Option<usize>,

        /// Match any token (OR) instead of all tokens (AND)
        #[arg(short = 'o', long = "or")]
        or_mode: bool,

        /// Index file path
        #[arg(short, long, default_value = "index.tkix")]
        index: PathBuf,

        /// Use memory-mapped loading (faster for repeated queries)
        #[arg(long)]
        mmap: bool,
    },

    /// Show index statistics
    Stats {
        /// Index file path
        #[arg(short, long, default_value = "index.tkix")]
        index: PathBuf,
    },

    /// Search for files by name using glob patterns
    Glob {
        /// Glob pattern to match filenames (e.g., "*.rs", "test_*.py")
        pattern: String,

        /// Index file path
        #[arg(short, long, default_value = "index.tkix")]
        index: PathBuf,

        /// Maximum results to return
        #[arg(short, long)]
        limit: Option<usize>,

        /// Use memory-mapped loading (faster for repeated queries)
        #[arg(long)]
        mmap: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Index {
            dir,
            output,
            extensions,
            exclude,
            max_size,
            legacy,
        } => {
            if legacy {
                cmd_index_legacy(dir, output, extensions, exclude, max_size)
            } else {
                cmd_index(dir, output, extensions, exclude, max_size)
            }
        }

        Commands::Query {
            query,
            fuzzy,
            path,
            glob,
            exclude,
            limit,
            or_mode,
            index,
            mmap,
        } => cmd_query(index, query, limit, or_mode, mmap, fuzzy, path, glob, exclude),

        Commands::Stats { index } => cmd_stats(index),

        Commands::Glob {
            pattern,
            index,
            limit,
            mmap,
        } => cmd_glob(index, pattern, limit, mmap),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn cmd_index(
    dir: PathBuf,
    output: PathBuf,
    extensions: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    max_size: u64,
) -> tokenizer::Result<()> {
    println!("Indexing directory: {}", dir.display());

    let mut config = ScanConfig::default();

    if let Some(exts) = extensions {
        config.extensions = exts;
    }

    if let Some(excl) = exclude {
        config.exclude_patterns.extend(excl);
    }

    config.max_file_size = max_size * 1024 * 1024;

    let start = Instant::now();
    let (path_index, exact_index, trigram_index) = scan_and_build_indexes(&dir, &config)?;
    let index_time = start.elapsed();

    println!(
        "Indexed {} files ({} exact tokens, {} trigrams) in {:.2}s",
        fmt_num(path_index.file_count()),
        fmt_num(exact_index.token_count()),
        fmt_num(trigram_index.trigram_count()),
        index_time.as_secs_f64()
    );

    let start = Instant::now();
    save_all(&path_index, &exact_index, &trigram_index, &output)?;
    let save_time = start.elapsed();

    // Calculate total size
    let paths_size = std::fs::metadata(paths_file(&output))
        .map(|m| m.len())
        .unwrap_or(0);
    let exact_size = std::fs::metadata(exact_file(&output))
        .map(|m| m.len())
        .unwrap_or(0);
    let trigram_size = std::fs::metadata(trigram_file(&output))
        .map(|m| m.len())
        .unwrap_or(0);
    let total_size = paths_size + exact_size + trigram_size;

    println!("Saved index files in {:.2}s:", save_time.as_secs_f64());
    println!(
        "  {} ({:.2} MB)",
        paths_file(&output).display(),
        paths_size as f64 / (1024.0 * 1024.0)
    );
    println!(
        "  {} ({:.2} MB)",
        exact_file(&output).display(),
        exact_size as f64 / (1024.0 * 1024.0)
    );
    println!(
        "  {} ({:.2} MB)",
        trigram_file(&output).display(),
        trigram_size as f64 / (1024.0 * 1024.0)
    );
    println!("  Total: {:.2} MB", total_size as f64 / (1024.0 * 1024.0));

    Ok(())
}

fn cmd_index_legacy(
    dir: PathBuf,
    output: PathBuf,
    extensions: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    max_size: u64,
) -> tokenizer::Result<()> {
    println!("Indexing directory (legacy mode): {}", dir.display());

    let mut config = ScanConfig::default();

    if let Some(exts) = extensions {
        config.extensions = exts;
    }

    if let Some(excl) = exclude {
        config.exclude_patterns.extend(excl);
    }

    config.max_file_size = max_size * 1024 * 1024;

    let start = Instant::now();
    let index = scan_and_index(&dir, &config)?;
    let index_time = start.elapsed();

    println!(
        "Indexed {} files with {} unique tokens in {:.2}s",
        fmt_num(index.file_count()),
        fmt_num(index.token_count()),
        index_time.as_secs_f64()
    );

    let start = Instant::now();
    save_index(&index, &output)?;
    let save_time = start.elapsed();

    let file_size = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);

    println!(
        "Saved index to {} ({:.2} MB) in {:.2}s",
        output.display(),
        file_size as f64 / (1024.0 * 1024.0),
        save_time.as_secs_f64()
    );

    Ok(())
}

fn cmd_query(
    index_path: PathBuf,
    query_str: String,
    limit: Option<usize>,
    or_mode: bool,
    use_mmap: bool,
    fuzzy: bool,
    path: Option<String>,
    glob: Option<Vec<String>>,
    exclude: Option<String>,
) -> tokenizer::Result<()> {
    // Default to exact mode (fuzzy = false means exact)

    // Check if we have the new split format or legacy format
    let has_split_format = paths_file(&index_path).exists();
    let has_legacy_format = index_path.exists() && !has_split_format;

    if !has_split_format && !has_legacy_format {
        return Err(TokenizerError::IndexNotFound(
            index_path.display().to_string(),
        ));
    }

    // If we have legacy format but trying to use new modes, error
    if has_legacy_format && !has_split_format {
        eprintln!("Warning: Legacy index format detected. Re-index with `tokenizer index` for --exact/--fuzzy support.");
        eprintln!("Falling back to legacy query...");

        // Fall back to legacy query
        let start = Instant::now();
        let index = if use_mmap {
            load_index_mmap(&index_path)?
        } else {
            load_index(&index_path)?
        };
        let load_time = start.elapsed();

        let start = Instant::now();
        let options = QueryOptions {
            limit,
            match_all: !or_mode,
            ..Default::default()
        };
        let result = query_with_options(&index, &query_str, &options);
        let query_time = start.elapsed();

        println!(
            "Query: \"{}\" ({} tokens, {} matched)",
            query_str, fmt_num(result.query_token_count), fmt_num(result.matched_token_count)
        );
        println!(
            "Found {} files in {:.3}ms (load: {:.3}ms)",
            fmt_num(result.files.len()),
            query_time.as_secs_f64() * 1000.0,
            load_time.as_secs_f64() * 1000.0
        );
        println!();

        for file in &result.files {
            println!("{}", file.display());
        }

        return Ok(());
    }

    // New split format query
    let start = Instant::now();
    let path_index = if use_mmap {
        load_paths_mmap(&paths_file(&index_path))?
    } else {
        load_paths(&paths_file(&index_path))?
    };
    let paths_load_time = start.elapsed();

    let options = QueryOptions {
        limit,
        match_all: !or_mode,
        path_contains: path,
        glob_patterns: glob,
        exclude,
    };

    let (result, mode_str, tokens_load_time) = if !fuzzy {
        let start = Instant::now();
        let exact_index = if use_mmap {
            load_exact_mmap(&exact_file(&index_path))?
        } else {
            load_exact(&exact_file(&index_path))?
        };
        let load_time = start.elapsed();

        // Validate index consistency
        validate_index_match(&path_index.header, &exact_index.header)?;

        let result = query_exact(&path_index, &exact_index, &query_str, &options);
        (result, "exact", load_time)
    } else {
        let start = Instant::now();
        let trigram_index = if use_mmap {
            load_trigram_mmap(&trigram_file(&index_path))?
        } else {
            load_trigram(&trigram_file(&index_path))?
        };
        let load_time = start.elapsed();

        // Validate index consistency
        validate_index_match(&path_index.header, &trigram_index.header)?;

        let result = query_fuzzy(&path_index, &trigram_index, &query_str, &options);
        (result, "fuzzy", load_time)
    };

    let total_load_time = paths_load_time + tokens_load_time;

    println!(
        "Query ({}): \"{}\" ({} tokens, {} matched)",
        mode_str, query_str, fmt_num(result.query_token_count), fmt_num(result.matched_token_count)
    );
    println!(
        "Found {} files in {:.3}ms (load: {:.3}ms)",
        fmt_num(result.files.len()),
        0.0, // Query time is fast, not separately tracked
        total_load_time.as_secs_f64() * 1000.0
    );
    println!();

    for file in &result.files {
        println!("{}", file.display());
    }

    Ok(())
}

fn cmd_stats(index_path: PathBuf) -> tokenizer::Result<()> {
    // Check for new split format first
    if paths_file(&index_path).exists() {
        let path_index = load_paths(&paths_file(&index_path))?;

        println!("Index Statistics (New Format)");
        println!("==============================");
        println!("Root path:     {}", path_index.root_path.display());
        println!("Files:         {}", fmt_num(path_index.file_count()));
        println!("Directories:   {}", fmt_num(path_index.directory_count()));
        println!(
            "Created:       {} (unix timestamp)",
            path_index.header.created_at
        );
        println!("Index ID:      {:02x?}", &path_index.header.index_id[..8]);

        // Load and show token counts
        if let Ok(exact_index) = load_exact(&exact_file(&index_path)) {
            println!("Exact tokens:  {}", fmt_num(exact_index.token_count()));
        }
        if let Ok(trigram_index) = load_trigram(&trigram_file(&index_path)) {
            println!("Trigrams:      {}", fmt_num(trigram_index.trigram_count()));
        }

        // File sizes
        let paths_size = std::fs::metadata(paths_file(&index_path))
            .map(|m| m.len())
            .unwrap_or(0);
        let exact_size = std::fs::metadata(exact_file(&index_path))
            .map(|m| m.len())
            .unwrap_or(0);
        let trigram_size = std::fs::metadata(trigram_file(&index_path))
            .map(|m| m.len())
            .unwrap_or(0);

        println!("\nFile sizes:");
        println!(
            "  Paths:   {:.2} MB",
            paths_size as f64 / (1024.0 * 1024.0)
        );
        println!(
            "  Exact:   {:.2} MB",
            exact_size as f64 / (1024.0 * 1024.0)
        );
        println!(
            "  Trigram: {:.2} MB",
            trigram_size as f64 / (1024.0 * 1024.0)
        );
        println!(
            "  Total:   {:.2} MB",
            (paths_size + exact_size + trigram_size) as f64 / (1024.0 * 1024.0)
        );

        return Ok(());
    }

    // Fall back to legacy format
    if !index_exists(&index_path) {
        return Err(TokenizerError::IndexNotFound(
            index_path.display().to_string(),
        ));
    }

    let index = load_index(&index_path)?;
    let metadata = index.metadata();

    let file_size = std::fs::metadata(&index_path)
        .map(|m| m.len())
        .unwrap_or(0);

    println!("Index Statistics (Legacy Format)");
    println!("=================================");
    println!("Version:       {}", metadata.version);
    println!("Root path:     {}", metadata.root_path.display());
    println!("Files:         {}", fmt_num(metadata.file_count));
    println!("Unique tokens: {}", fmt_num(metadata.token_count));
    println!(
        "Index size:    {:.2} MB",
        file_size as f64 / (1024.0 * 1024.0)
    );
    println!("Created:       {} (unix timestamp)", metadata.created_at);

    Ok(())
}

fn cmd_glob(
    index_path: PathBuf,
    pattern: String,
    limit: Option<usize>,
    use_mmap: bool,
) -> tokenizer::Result<()> {
    // Check for new split format first (only needs paths file)
    if paths_file(&index_path).exists() {
        let start = Instant::now();
        let path_index = if use_mmap {
            load_paths_mmap(&paths_file(&index_path))?
        } else {
            load_paths(&paths_file(&index_path))?
        };
        let load_time = start.elapsed();

        let start = Instant::now();
        let options = GlobOptions { limit };
        let result = glob_files(&path_index, &pattern, &options)?;
        let glob_time = start.elapsed();

        println!(
            "Pattern: \"{}\" (scanned {} files)",
            result.pattern, fmt_num(result.files_scanned)
        );
        println!(
            "Found {} files in {:.3}ms (load: {:.3}ms)",
            fmt_num(result.files.len()),
            glob_time.as_secs_f64() * 1000.0,
            load_time.as_secs_f64() * 1000.0
        );
        println!();

        for file in &result.files {
            println!("{}", file.display());
        }

        return Ok(());
    }

    // Fall back to legacy format
    if !index_exists(&index_path) {
        return Err(TokenizerError::IndexNotFound(
            index_path.display().to_string(),
        ));
    }

    let start = Instant::now();
    let index = if use_mmap {
        load_index_mmap(&index_path)?
    } else {
        load_index(&index_path)?
    };
    let load_time = start.elapsed();

    let start = Instant::now();
    let options = GlobOptions { limit };
    let result = glob_files(&index, &pattern, &options)?;
    let glob_time = start.elapsed();

    println!(
        "Pattern: \"{}\" (scanned {} files)",
        result.pattern, fmt_num(result.files_scanned)
    );
    println!(
        "Found {} files in {:.3}ms (load: {:.3}ms)",
        fmt_num(result.files.len()),
        glob_time.as_secs_f64() * 1000.0,
        load_time.as_secs_f64() * 1000.0
    );
    println!();

    for file in &result.files {
        println!("{}", file.display());
    }

    Ok(())
}

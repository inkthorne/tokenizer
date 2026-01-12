use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Instant;
use tokenizer::{
    glob_files, index_exists, load_index, load_index_mmap, query_with_options, save_index,
    scan_and_index, GlobOptions, QueryOptions, ScanConfig, TokenizerError,
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

        /// Output index file path
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
    },

    /// Query an existing index
    Query {
        /// Index file path
        #[arg(short, long, default_value = "index.tkix")]
        index: PathBuf,

        /// Search query
        query: String,

        /// Maximum results to return
        #[arg(short, long)]
        limit: Option<usize>,

        /// Match any token (OR) instead of all tokens (AND)
        #[arg(long)]
        any: bool,

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
        } => cmd_index(dir, output, extensions, exclude, max_size),

        Commands::Query {
            index,
            query,
            limit,
            any,
            mmap,
        } => cmd_query(index, query, limit, any, mmap),

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
        config.exclude_patterns = excl;
    }

    config.max_file_size = max_size * 1024 * 1024;

    let start = Instant::now();
    let index = scan_and_index(&dir, &config)?;
    let index_time = start.elapsed();

    println!(
        "Indexed {} files with {} unique tokens in {:.2}s",
        index.file_count(),
        index.token_count(),
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
    any: bool,
    use_mmap: bool,
) -> tokenizer::Result<()> {
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
    let options = QueryOptions {
        limit,
        match_all: !any,
    };
    let result = query_with_options(&index, &query_str, &options);
    let query_time = start.elapsed();

    println!(
        "Query: \"{}\" ({} tokens, {} matched)",
        query_str, result.query_token_count, result.matched_token_count
    );
    println!(
        "Found {} files in {:.3}ms (load: {:.3}ms)",
        result.files.len(),
        query_time.as_secs_f64() * 1000.0,
        load_time.as_secs_f64() * 1000.0
    );
    println!();

    for file in &result.files {
        println!("{}", file.display());
    }

    Ok(())
}

fn cmd_stats(index_path: PathBuf) -> tokenizer::Result<()> {
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

    println!("Index Statistics");
    println!("================");
    println!("Version:       {}", metadata.version);
    println!("Root path:     {}", metadata.root_path.display());
    println!("Files:         {}", metadata.file_count);
    println!("Unique tokens: {}", metadata.token_count);
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
        result.pattern, result.files_scanned
    );
    println!(
        "Found {} files in {:.3}ms (load: {:.3}ms)",
        result.files.len(),
        glob_time.as_secs_f64() * 1000.0,
        load_time.as_secs_f64() * 1000.0
    );
    println!();

    for file in &result.files {
        println!("{}", file.display());
    }

    Ok(())
}

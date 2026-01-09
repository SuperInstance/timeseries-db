//! timeseries-db command-line interface

use clap::{Parser, Subcommand};
use anyhow::Result;

#[derive(Parser)]
#[command(name = "timeseries-db")]
#[command(about = "High-performance time-series database", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Open an interactive shell
    Shell {
        /// Database path
        #[arg(short, long, default_value = "./data")]
        path: String,
    },

    /// Import data from a file
    Import {
        /// Database path
        #[arg(short, long)]
        db: String,

        /// Input file (JSON)
        input: String,
    },

    /// Export data to a file
    Export {
        /// Database path
        #[arg(short, long)]
        db: String,

        /// Output file (JSON)
        output: String,

        /// Metric name
        #[arg(short, long)]
        metric: String,

        /// Start time (Unix nanoseconds)
        #[arg(short, long)]
        start: i64,

        /// End time (Unix nanoseconds)
        #[arg(short, long)]
        end: i64,
    },

    /// Query the database
    Query {
        /// Database path
        #[arg(short, long)]
        db: String,

        /// Metric name
        #[arg(short, long)]
        metric: String,

        /// Start time (relative, e.g., "24h", "7d")
        #[arg(short, long)]
        start: String,

        /// Tag filters (e.g., "user=casey")
        #[arg(short, long)]
        tags: Vec<String>,
    },

    /// Show database statistics
    Stats {
        /// Database path
        #[arg(short, long, default_value = "./data")]
        db: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Shell { path } => {
            println!("Opening timeseries-db shell: {}", path);
            println!("(Shell not implemented yet)");
        }

        Commands::Import { db, input } => {
            println!("Importing data from {} to {}", input, db);
            println!("(Import not implemented yet)");
        }

        Commands::Export { db, output, metric, start, end } => {
            println!("Exporting {} from {} ({} to {}) to {}", metric, db, start, end, output);
            println!("(Export not implemented yet)");
        }

        Commands::Query { db, metric, start, tags } => {
            println!("Querying {} from {} (start: {}, tags: {:?})", metric, db, start, tags);
            println!("(Query not implemented yet)");
        }

        Commands::Stats { db } => {
            println!("Statistics for database: {}", db);
            println!("(Stats not implemented yet)");
        }
    }

    Ok(())
}

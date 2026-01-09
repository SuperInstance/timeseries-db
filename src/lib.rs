//! timeseries-db: High-performance time-series database
//!
//! This library provides efficient storage and querying of time-series data,
//! optimized for high write throughput (>1M points/sec) and fast range queries.
//!
//! # Example
//!
//! ```rust
//! use timeseries_db::{TimeSeriesDB, Point, Query};
//!
//! # fn main() -> anyhow::Result<()> {
//! // Open database
//! let db = TimeSeriesDB::open("makelog.db")?;
//!
//! // Write a point
//! db.write(Point {
//!     timestamp: 1736359200000000000,  // 2026-01-08 12:00:00 UTC
//!     metric: "activity.completed".into(),
//!     value: 1.0,
//!     tags: map!("user" => "casey", "project" => "equilibrium-tokens"),
//! })?;
//!
//! // Query last 24 hours
//! let points = db.query(Query {
//!     metric: "activity.completed".into(),
//!     start: now() - 24 * 3600 * 1_000_000_000,
//!     end: now(),
//!     tags: Some(map!("user" => "casey")),
//!     ..Default::default()
//! })?;
//!
//! println!("Found {} points", points.len());
//! # Ok(())
//! # }
//! ```

mod point;
mod memtable;
mod wal;
mod sstable;
mod query;
mod compression;
mod database;

pub use point::Point;
pub use memtable::Memtable;
pub use wal::WAL;
pub use sstable::SSTable;
pub use query::{Query, Aggregation, Downsample, GroupBy, Column};
pub use compression::{compress_gorilla, decompress_gorilla};
pub use database::TimeSeriesDB;

pub mod error;
pub use error::{Result, Error};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_usage() {
        // TODO: Add basic usage test
    }
}

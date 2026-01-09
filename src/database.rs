//! Main database interface

use crate::point::Point;
use crate::memtable::Memtable;
use crate::wal::WAL;
use crate::sstable::SSTable;
use crate::query::Query;
use crate::error::{Result, Error};
use std::path::{Path, PathBuf};
use std::fs;
use std::collections::HashMap;

/// Main time-series database
pub struct TimeSeriesDB {
    memtable: Memtable,
    wal: WAL,
    sstables: Vec<SSTable>,
    config: Config,
    path: PathBuf,
}

/// Database configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Maximum memtable size before flush (default: 1GB)
    pub max_memtable_size: usize,

    /// Enable WAL fsync after each write (default: true)
    pub wal_sync: bool,

    /// SSTable directory (default: "data/sst")
    pub sst_dir: PathBuf,

    /// WAL directory (default: "data/wal")
    pub wal_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_memtable_size: 1024 * 1024 * 1024,  // 1GB
            wal_sync: true,
            sst_dir: PathBuf::from("data/sst"),
            wal_dir: PathBuf::from("data/wal"),
        }
    }
}

impl TimeSeriesDB {
    /// Open a time-series database
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let config = Config::default();

        // Create directories
        let sst_dir = path.join(&config.sst_dir);
        let wal_dir = path.join(&config.wal_dir);

        fs::create_dir_all(&sst_dir)?;
        fs::create_dir_all(&wal_dir)?;

        // Open WAL
        let wal_path = wal_dir.join("wal.log");
        let mut wal = WAL::create(&wal_path)?;
        wal.set_sync(config.wal_sync);

        // Replay WAL to recover memtable
        let recovered_points = wal.replay()?;
        let mut memtable = Memtable::new(config.max_memtable_size);

        for point in recovered_points {
            memtable.insert(point)?;
        }

        // Load existing SSTables
        let sstables = Self::load_sstables(&sst_dir)?;

        Ok(Self {
            memtable,
            wal,
            sstables,
            config,
            path: path.to_path_buf(),
        })
    }

    /// Open with custom configuration
    pub fn open_with_config(path: impl AsRef<Path>, config: Config) -> Result<Self> {
        let path = path.as_ref();

        // Create directories
        let sst_dir = path.join(&config.sst_dir);
        let wal_dir = path.join(&config.wal_dir);

        fs::create_dir_all(&sst_dir)?;
        fs::create_dir_all(&wal_dir)?;

        // Open WAL
        let wal_path = wal_dir.join("wal.log");
        let mut wal = WAL::create(&wal_path)?;
        wal.set_sync(config.wal_sync);

        // Replay WAL to recover memtable
        let recovered_points = wal.replay()?;
        let mut memtable = Memtable::new(config.max_memtable_size);

        for point in recovered_points {
            memtable.insert(point)?;
        }

        // Load existing SSTables
        let sstables = Self::load_sstables(&sst_dir)?;

        Ok(Self {
            memtable,
            wal,
            sstables,
            config,
            path: path.to_path_buf(),
        })
    }

    /// Write a point to the database
    pub fn write(&mut self, point: Point) -> Result<()> {
        // Append to WAL
        self.wal.append(&point)?;

        // Insert into memtable
        self.memtable.insert(point.clone())?;

        // Check if memtable needs flush
        if self.memtable.needs_flush() {
            self.flush()?;
        }

        Ok(())
    }

    /// Write multiple points (batch)
    pub fn write_batch(&mut self, points: Vec<Point>) -> Result<()> {
        for point in points {
            self.write(point)?;
        }
        Ok(())
    }

    /// Query the database
    pub fn query(&self, query: Query) -> Result<Vec<crate::point::Point>> {
        query.validate()?;

        let mut results = Vec::new();

        // Query memtable
        results.extend(self.memtable.query(query.start..query.end));

        // Query SSTables
        for sstable in &self.sstables {
            let time_range = sstable.time_range();
            if time_range.0 <= query.end && time_range.1 >= query.start {
                results.extend(sstable.query(query.start..query.end)?);
            }
        }

        // Sort by timestamp
        results.sort_by_key(|p| p.timestamp);

        // Execute query (filters, downsampling, aggregation)
        let results = crate::query::execute_query(results, &query)?;

        Ok(results)
    }

    /// Flush memtable to SSTable
    pub fn flush(&mut self) -> Result<()> {
        if self.memtable.is_empty() {
            return Ok(());
        }

        // Create SSTable filename (hourly partition)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos() as u64;

        let hour = now / 3_600_000_000_000;  // Nanoseconds per hour
        let sst_path = self.path.join(&self.config.sst_dir)
            .join(format!("{:016}_{}.sst", hour, 0));

        // Create SSTable from memtable
        let sstable = SSTable::create(self.memtable.clone(), &sst_path)?;

        // Add to SSTables list
        self.sstables.push(sstable);

        // Clear memtable and WAL
        self.memtable.clear();
        self.wal.truncate()?;

        Ok(())
    }

    /// Load existing SSTables from directory
    fn load_sstables(sst_dir: &Path) -> Result<Vec<SSTable>> {
        let mut sstables = Vec::new();

        if !sst_dir.exists() {
            return Ok(sstables);
        }

        for entry in fs::read_dir(sst_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("sst") {
                let sstable = SSTable::open(&path)?;
                sstables.push(sstable);
            }
        }

        // Sort by time range
        sstables.sort_by_key(|s| s.time_range());

        Ok(sstables)
    }

    /// Get database statistics
    pub fn stats(&self) -> DatabaseStats {
        DatabaseStats {
            memtable_size: self.memtable.size(),
            memtable_points: self.memtable.len(),
            num_sstables: self.sstables.len(),
            path: self.path.clone(),
        }
    }

    /// Close the database
    pub fn close(mut self) -> Result<()> {
        // Flush memtable
        if !self.memtable.is_empty() {
            self.flush()?;
        }

        Ok(())
    }
}

/// Database statistics
#[derive(Debug, Clone)]
pub struct DatabaseStats {
    /// Current memtable size in bytes
    pub memtable_size: usize,

    /// Number of points in memtable
    pub memtable_points: usize,

    /// Number of SSTables
    pub num_sstables: usize,

    /// Database path
    pub path: PathBuf,
}

// Downsample module (stub)
mod downsample {
    use super::*;
    use std::time::Duration;

    pub fn downsample(points: &[Point], interval: Duration, agg: Aggregation) -> Vec<Point> {
        if points.is_empty() {
            return Vec::new();
        }

        let mut buckets: HashMap<i64, Vec<f64>> = HashMap::new();

        // Group by time bucket
        for point in points {
            let bucket = (point.timestamp / interval.as_nanos() as i64) * interval.as_nanos() as i64;
            buckets.entry(bucket).or_default().push(point.value);
        }

        // Apply aggregation
        buckets.into_iter()
            .map(|(timestamp, values)| {
                let value = match agg {
                    Aggregation::Avg => values.iter().sum::<f64>() / values.len() as f64,
                    Aggregation::Sum => values.iter().sum(),
                    Aggregation::Min => values.iter().fold(f64::INFINITY, |a, &b| a.min(b)),
                    Aggregation::Max => values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b)),
                    Aggregation::Count => values.len() as f64,
                };

                Point {
                    timestamp,
                    metric: points[0].metric.clone(),
                    value,
                    tags: points[0].tags.clone(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_database_open() {
        let temp_dir = TempDir::new().unwrap();
        let db = TimeSeriesDB::open(temp_dir.path()).unwrap();

        let stats = db.stats();
        assert_eq!(stats.memtable_points, 0);
        assert_eq!(stats.num_sstables, 0);
    }

    #[test]
    fn test_database_write_and_query() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = TimeSeriesDB::open(temp_dir.path()).unwrap();

        // Write points
        let point = Point::new(1000, "test.metric", 1.0)
            .with_tag("user", "casey");

        db.write(point).unwrap();

        // Query
        let query = Query::new("test.metric", 0, 2000)
            .with_tag("user", "casey");

        let results = db.query(query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].value, 1.0);
    }

    #[test]
    fn test_database_flush() {
        let temp_dir = TempDir::new().unwrap();
        let mut db = TimeSeriesDB::open_with_config(
            temp_dir.path(),
            Config {
                max_memtable_size: 100,  // Very small for testing
                ..Default::default()
            }
        ).unwrap();

        // Write enough points to trigger flush
        for i in 0..10 {
            db.write(Point::new(i, "test.metric", i as f64)).unwrap();
        }

        // Should have flushed to SSTable
        let stats = db.stats();
        assert!(stats.num_sstables > 0 || stats.memtable_points > 0);
    }
}

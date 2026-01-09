//! In-memory time-series storage

use crate::point::Point;
use crate::error::{Result, Error};
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

/// In-memory write buffer with time-based indexing
pub struct Memtable {
    /// Time-ordered storage: timestamp -> points
    data: BTreeMap<i64, Vec<Point>>,

    /// Current size in bytes
    size: usize,

    /// Maximum size before flush (default: 1GB)
    max_size: usize,
}

impl Memtable {
    /// Create a new memtable
    pub fn new(max_size: usize) -> Self {
        Self {
            data: BTreeMap::new(),
            size: 0,
            max_size,
        }
    }

    /// Create a memtable with default max size (1GB)
    pub fn default() -> Self {
        Self::new(1024 * 1024 * 1024)  // 1GB
    }

    /// Insert a point into the memtable
    pub fn insert(&mut self, point: Point) -> Result<()> {
        point.validate()?;

        let point_size = point.size();
        self.size += point_size;

        self.data
            .entry(point.timestamp)
            .or_default()
            .push(point);

        Ok(())
    }

    /// Query points in a time range
    pub fn query(&self, range: Range<i64>) -> Vec<Point> {
        self.data
            .range(range)
            .flat_map(|(_, points)| points.iter().cloned())
            .collect()
    }

    /// Check if the memtable needs to be flushed
    pub fn needs_flush(&self) -> bool {
        self.size >= self.max_size
    }

    /// Get the current size in bytes
    pub fn size(&self) -> usize {
        self.size
    }

    /// Get the number of points in the memtable
    pub fn len(&self) -> usize {
        self.data.values().map(|v| v.len()).sum()
    }

    /// Check if the memtable is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Clear the memtable (after flush)
    pub fn clear(&mut self) {
        self.data.clear();
        self.size = 0;
    }

    /// Get the time range of points in the memtable
    pub fn time_range(&self) -> Option<(i64, i64)> {
        if self.data.is_empty() {
            return None;
        }

        let min = *self.data.keys().next()?;
        let max = *self.data.keys().next_back()?;
        Some((min, max))
    }

    /// Convert memtable to a sorted vector of points
    pub fn to_vec(&self) -> Vec<Point> {
        self.data
            .values()
            .flat_map(|points| points.iter().cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memtable_insert() {
        let mut memtable = Memtable::default();

        let point = Point::new(1736359200000000000, "test.metric", 1.0);
        memtable.insert(point.clone()).unwrap();

        assert_eq!(memtable.len(), 1);
        assert!(!memtable.is_empty());
    }

    #[test]
    fn test_memtable_query() {
        let mut memtable = Memtable::default();

        memtable.insert(Point::new(1000, "test.metric", 1.0)).unwrap();
        memtable.insert(Point::new(2000, "test.metric", 2.0)).unwrap();
        memtable.insert(Point::new(3000, "test.metric", 3.0)).unwrap();

        let results = memtable.query(1500..2500);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].value, 2.0);
    }

    #[test]
    fn test_memtable_flush() {
        let mut memtable = Memtable::new(100);  // Small max size

        // Insert points until flush is needed
        for i in 0..10 {
            memtable.insert(Point::new(i, "test.metric", i as f64)).unwrap();
        }

        assert!(memtable.needs_flush());
    }

    #[test]
    fn test_memtable_time_range() {
        let mut memtable = Memtable::default();

        memtable.insert(Point::new(1000, "test.metric", 1.0)).unwrap();
        memtable.insert(Point::new(2000, "test.metric", 2.0)).unwrap();
        memtable.insert(Point::new(3000, "test.metric", 3.0)).unwrap();

        let range = memtable.time_range();
        assert_eq!(range, Some((1000, 3000)));
    }

    #[test]
    fn test_memtable_clear() {
        let mut memtable = Memtable::default();

        memtable.insert(Point::new(1000, "test.metric", 1.0)).unwrap();
        memtable.clear();

        assert!(memtable.is_empty());
        assert_eq!(memtable.size(), 0);
    }
}

//! Time-series data point

use crate::error::{Result, Error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Read, Write};

/// A single time-series data point
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// Unix timestamp in nanoseconds (UTC)
    pub timestamp: i64,

    /// Metric name (e.g., "activity.completed")
    pub metric: String,

    /// Measurement value
    pub value: f64,

    /// Dimensions/tags (e.g., {"user": "casey", "project": "equilibrium-tokens"})
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

impl Point {
    /// Create a new point
    pub fn new(
        timestamp: i64,
        metric: impl Into<String>,
        value: f64,
    ) -> Self {
        Self {
            timestamp,
            metric: metric.into(),
            value,
            tags: HashMap::new(),
        }
    }

    /// Add a tag to the point
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Add multiple tags to the point
    pub fn with_tags(mut self, tags: HashMap<String, String>) -> Self {
        self.tags = tags;
        self
    }

    /// Validate the point
    pub fn validate(&self) -> Result<()> {
        if self.timestamp < 0 {
            return Err(Error::InvalidTimestamp(self.timestamp));
        }

        if self.metric.is_empty() {
            return Err(Error::InvalidMetricName("empty metric name".into()));
        }

        if self.metric.len() > 256 {
            return Err(Error::InvalidMetricName("metric name too long".into()));
        }

        if !self.value.is_finite() {
            return Err(Error::Query(format!("invalid value: {}", self.value)));
        }

        Ok(())
    }

    /// Calculate the size of this point in bytes (when serialized)
    pub fn size(&self) -> usize {
        // Timestamp: 8 bytes
        // Value: 8 bytes
        // Metric: 2 bytes (length) + metric.len()
        // Tags: 2 bytes (count) + sum of each tag
        let mut size = 8 + 8 + 2 + self.metric.len() + 2;

        for (key, value) in &self.tags {
            size += 2 + key.len() + 2 + value.len();
        }

        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_creation() {
        let point = Point::new(1736359200000000000, "activity.completed", 1.0)
            .with_tag("user", "casey")
            .with_tag("project", "equilibrium-tokens");

        assert_eq!(point.timestamp, 1736359200000000000);
        assert_eq!(point.metric, "activity.completed");
        assert_eq!(point.value, 1.0);
        assert_eq!(point.tags.len(), 2);
        assert_eq!(point.tags.get("user"), Some(&"casey".to_string()));
    }

    #[test]
    fn test_point_validation() {
        let point = Point::new(1736359200000000000, "activity.completed", 1.0);
        assert!(point.validate().is_ok());

        let invalid_point = Point::new(-1, "activity.completed", 1.0);
        assert!(invalid_point.validate().is_err());

        let invalid_metric = Point::new(1736359200000000000, "", 1.0);
        assert!(invalid_metric.validate().is_err());

        let invalid_value = Point::new(1736359200000000000, "activity.completed", f64::NAN);
        assert!(invalid_value.validate().is_err());
    }

    #[test]
    fn test_point_size() {
        let point = Point::new(1736359200000000000, "activity.completed", 1.0)
            .with_tag("user", "casey");

        // 8 (timestamp) + 8 (value) + 2 + 18 (metric) + 2 + 2 + 4 (user) + 2 + 5 (casey) = 51 bytes
        assert_eq!(point.size(), 51);
    }
}

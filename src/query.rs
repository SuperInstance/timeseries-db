//! Query types and execution

use crate::point::Point;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Query for time-series data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    /// Metric name (e.g., "activity.completed")
    pub metric: String,

    /// Start timestamp (Unix nanoseconds)
    pub start: i64,

    /// End timestamp (Unix nanoseconds)
    pub end: i64,

    /// Tag filters (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<HashMap<String, String>>,

    /// Columns to return
    #[serde(default)]
    pub columns: Vec<Column>,

    /// Group by clause
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_by: Option<Vec<GroupBy>>,

    /// Downsampling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downsample: Option<Downsample>,

    /// Aggregations
    #[serde(default)]
    pub aggregations: Vec<Aggregation>,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            metric: String::new(),
            start: 0,
            end: i64::MAX,
            tags: None,
            columns: vec![Column::All],
            group_by: None,
            downsample: None,
            aggregations: Vec::new(),
        }
    }
}

impl Query {
    /// Create a new query
    pub fn new(metric: impl Into<String>, start: i64, end: i64) -> Self {
        Self {
            metric: metric.into(),
            start,
            end,
            ..Default::default()
        }
    }

    /// Add tag filter
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.get_or_insert_with(HashMap::new)
            .insert(key.into(), value.into());
        self
    }

    /// Add tag filters
    pub fn with_tags(mut self, tags: HashMap<String, String>) -> Self {
        self.tags = Some(tags);
        self
    }

    /// Add downsampling
    pub fn with_downsample(mut self, interval: Duration, aggregation: Aggregation) -> Self {
        self.downsample = Some(Downsample {
            interval,
            aggregation,
        });
        self
    }

    /// Add group by
    pub fn with_group_by(mut self, group_by: Vec<GroupBy>) -> Self {
        self.group_by = Some(group_by);
        self
    }

    /// Add aggregation
    pub fn with_aggregation(mut self, aggregation: Aggregation) -> Self {
        self.aggregations.push(aggregation);
        self
    }

    /// Validate the query
    pub fn validate(&self) -> Result<()> {
        if self.metric.is_empty() {
            return Err(crate::error::Error::InvalidMetricName("empty metric name".into()));
        }

        if self.start < 0 || self.end < 0 {
            return Err(crate::error::Error::InvalidTimestamp(self.start));
        }

        if self.start >= self.end {
            return Err(crate::error::Error::Query("start must be before end".into()));
        }

        Ok(())
    }
}

/// Column to select
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Column {
    /// All columns (*)
    All,
    /// Timestamp only
    Timestamp,
    /// Value only
    Value,
    /// Metric only
    Metric,
    /// Specific tag
    Tag(String),
}

/// Group by clause
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GroupBy {
    /// Group by time interval
    Time(Duration),
    /// Group by tag
    Tag(String),
}

/// Downsampling configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Downsample {
    /// Time interval for downsampling
    pub interval: Duration,

    /// Aggregation function
    pub aggregation: Aggregation,
}

/// Aggregation function
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Aggregation {
    /// Average
    Avg,
    /// Sum
    Sum,
    /// Minimum
    Min,
    /// Maximum
    Max,
    /// Count
    Count,
}

/// Execute a query on a set of points
pub fn execute_query(points: Vec<Point>, query: &Query) -> Result<Vec<Point>> {
    let mut results = points;

    // Apply tag filters
    if let Some(tags) = &query.tags {
        results.retain(|p| {
            tags.iter().all(|(k, v)| p.tags.get(k) == Some(v))
        });
    }

    // Apply downsampling
    if let Some(downsample) = &query.downsample {
        results = crate::downsample::downsample(&results, downsample.interval, downsample.aggregation);
    }

    // Apply aggregations
    if !query.aggregations.is_empty() {
        results = aggregate(&results, &query.aggregations)?;
    }

    // Apply group by
    if let Some(group_by) = &query.group_by {
        results = group_by_clause(&results, group_by)?;
    }

    Ok(results)
}

/// Aggregate points
fn aggregate(points: &[Point], aggregations: &[Aggregation]) -> Result<Vec<Point>> {
    if points.is_empty() {
        return Ok(Vec::new());
    }

    let mut result_points = Vec::new();

    for agg in aggregations {
        let value = match agg {
            Aggregation::Avg => {
                let sum: f64 = points.iter().map(|p| p.value).sum();
                sum / points.len() as f64
            },
            Aggregation::Sum => {
                points.iter().map(|p| p.value).sum()
            },
            Aggregation::Min => {
                points.iter().map(|p| p.value).fold(f64::INFINITY, |a, b| a.min(b))
            },
            Aggregation::Max => {
                points.iter().map(|p| p.value).fold(f64::NEG_INFINITY, |a, b| a.max(b))
            },
            Aggregation::Count => {
                points.len() as f64
            },
        };

        // Create result point
        let result_point = Point {
            timestamp: points[0].timestamp,  // Use first timestamp
            metric: points[0].metric.clone(),
            value,
            tags: points[0].tags.clone(),
        };

        result_points.push(result_point);
    }

    Ok(result_points)
}

/// Group by clause
fn group_by_clause(points: Vec<Point>, group_by: &[GroupBy]) -> Result<Vec<Point>> {
    use std::collections::HashMap;

    let mut groups: HashMap<GroupKey, Vec<Point>> = HashMap::new();

    for point in points {
        let key = match &group_by[0] {
            GroupBy::Time(interval) => {
                let bucket = (point.timestamp / interval.as_nanos() as i64) * interval.as_nanos() as i64;
                GroupKey::Time(bucket)
            },
            GroupBy::Tag(tag) => {
                let value = point.tags.get(tag).cloned().unwrap_or_default();
                GroupKey::Tag(tag.clone(), value)
            },
        };

        groups.entry(key).or_default().push(point);
    }

    // Aggregate each group
    let mut results = Vec::new();
    for (key, group_points) in groups {
        let value: f64 = group_points.iter().map(|p| p.value).sum();

        let result_point = Point {
            timestamp: match key {
                GroupKey::Time(ts) => ts,
                GroupKey::Tag(_, _) => group_points[0].timestamp,
            },
            metric: group_points[0].metric.clone(),
            value,
            tags: group_points[0].tags.clone(),
        };

        results.push(result_point);
    }

    results.sort_by_key(|p| p.timestamp);
    Ok(results)
}

/// Group key for GROUP BY
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum GroupKey {
    Time(i64),
    Tag(String, String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_creation() {
        let query = Query::new("test.metric", 0, 1000)
            .with_tag("user", "casey")
            .with_downsample(Duration::from_secs(60), Aggregation::Avg);

        assert_eq!(query.metric, "test.metric");
        assert_eq!(query.tags.as_ref().unwrap().len(), 1);
        assert!(query.downsample.is_some());
    }

    #[test]
    fn test_query_validation() {
        let query = Query::new("test.metric", 0, 1000);
        assert!(query.validate().is_ok());

        let invalid_query = Query::new("", 0, 1000);
        assert!(invalid_query.validate().is_err());

        let invalid_range = Query::new("test.metric", 1000, 0);
        assert!(invalid_range.validate().is_err());
    }
}

# timeseries-db Query Language

## Overview

The timeseries-db query language is a SQL-like syntax designed for time-series data. It focuses on time-based filtering, aggregation, and downsampling.

**Design Goals**:
- **Familiarity**: SQL-like syntax for developers
- **Expressiveness**: Support for common time-series queries
- **Performance**: Optimized for range queries and aggregations
- **Simplicity**: Limited feature set (no JOINs, no subqueries)

---

## Basic Syntax

### SELECT Statement

```sql
SELECT [columns] FROM [metric]
WHERE [conditions]
GROUP BY [grouping]
DOWNSAMPLE [interval], [aggregation];
```

### Components

- **SELECT**: Columns to return (use `*` for all columns)
- **FROM**: Metric name (e.g., `activity.completed`)
- **WHERE**: Time range and tag filters
- **GROUP BY**: Grouping by time interval and/or tags
- **DOWNSAMPLE**: Reduce data resolution with aggregation

---

## SELECT Clause

### Select All Columns

```sql
SELECT * FROM activity.completed
WHERE time > now() - 24h;
```

**Returns**: All points with all fields (timestamp, metric, value, tags)

### Select Specific Columns

```sql
SELECT timestamp, value FROM activity.completed
WHERE time > now() - 24h;
```

**Returns**: Points with only timestamp and value fields

### Aggregation Functions

```sql
SELECT mean(value), max(value), min(value) FROM activity.completed
WHERE time > now() - 24h;
```

**Supported Functions**:
- `mean(value)`: Average value
- `max(value)`: Maximum value
- `min(value)`: Minimum value
- `sum(value)`: Sum of values
- `count(value)`: Number of points

---

## WHERE Clause

### Time Range Filtering

#### Relative Time

```sql
SELECT * FROM activity.completed
WHERE time > now() - 24h;
```

**Time Units**:
- `s` / `sec`: Seconds
- `m` / `min`: Minutes
- `h` / `hour`: Hours
- `d` / `day`: Days
- `w` / `week`: Weeks
- `mo` / `month`: Months (30 days)
- `y` / `year`: Years (365 days)

**Examples**:
```sql
-- Last 1 hour
WHERE time > now() - 1h

-- Last 7 days
WHERE time > now() - 7d

-- Last 30 days
WHERE time > now() - 30d

-- Last 1 year
WHERE time > now() - 1y
```

#### Absolute Time

```sql
SELECT * FROM activity.completed
WHERE time > 1736359200000000000
  AND time < 1736445600000000000;
```

**Format**: Unix nanoseconds (UTC)

**Converters**:
```bash
# Date to nanoseconds
date -d "2026-01-08 12:00:00" +%s%N  # Linux

# Nanoseconds to date
date -d @1736359200                   # Linux (seconds only)
```

#### Time Range Shorthand

```sql
-- Last 24 hours (shorthand for WHERE time > now() - 24h AND time < now())
SELECT * FROM activity.completed
WHERE time > now() - 24h;

-- Explicit time range
SELECT * FROM activity.completed
WHERE time > now() - 24h
  AND time < now();
```

### Tag Filtering

#### Equality

```sql
SELECT * FROM activity.completed
WHERE time > now() - 24h
  AND user = 'casey';
```

#### Multiple Tags

```sql
SELECT * FROM activity.completed
WHERE time > now() - 24h
  AND user = 'casey'
  AND project = 'equilibrium-tokens';
```

#### IN Operator

```sql
SELECT * FROM activity.completed
WHERE time > now() - 24h
  AND user IN ('casey', 'alice', 'bob');
```

#### Regular Expression (Future)

```sql
-- Not implemented in MVP
SELECT * FROM activity.completed
WHERE time > now() - 24h
  AND project =~ 'equilibrium-.*';
```

---

## GROUP BY Clause

### Group by Time Interval

```sql
SELECT mean(value) FROM activity.completed
WHERE time > now() - 7d
GROUP BY time(1h);
```

**Result**: One data point per hour with the average value

**Time Intervals**:
- `time(1s)`: 1 second
- `time(1m)`: 1 minute
- `time(5m)`: 5 minutes
- `time(1h)`: 1 hour
- `time(1d)`: 1 day
- `time(1w)`: 1 week

### Group by Tag

```sql
SELECT mean(value), user FROM activity.completed
WHERE time > now() - 7d
GROUP BY user;
```

**Result**: One data point per user with the average value

### Group by Time and Tag

```sql
SELECT mean(value), user FROM activity.completed
WHERE time > now() - 7d
GROUP BY time(1h), user;
```

**Result**: One data point per hour per user

---

## DOWNSAMPLE Clause

### Basic Downsampling

```sql
SELECT downsample(value, 1h, avg) FROM activity.completed
WHERE time > now() - 30d;
```

**Parameters**:
1. `value`: Column to downsample
2. `1h`: Target interval (1 hour)
3. `avg`: Aggregation function

**Aggregation Functions**:
- `avg`: Average
- `sum`: Sum
- `min`: Minimum
- `max`: Maximum
- `count`: Count

### Downsampling with GROUP BY

```sql
SELECT downsample(value, 1h, avg) FROM activity.completed
WHERE time > now() - 30d
GROUP BY user;
```

**Result**: One data point per hour per user

---

## Examples

### Example 1: Last 24 Hours of Activity

**Query**:
```sql
SELECT * FROM activity.completed
WHERE time > now() - 24h
  AND user = 'casey';
```

**Rust Equivalent**:
```rust
let points = db.query(Query {
    metric: "activity.completed".into(),
    start: now() - 24 * 3600 * 1_000_000_000,
    end: now(),
    tags: Some(map!("user" => "casey")),
    ..Default::default()
})?;
```

**Result**: Vector of points (timestamp, value, tags)

---

### Example 2: Daily Activity Summary

**Query**:
```sql
SELECT count(value), sum(value) FROM activity.completed
WHERE time > now() - 24h
GROUP BY user;
```

**Rust Equivalent**:
```rust
let points = db.query(Query {
    metric: "activity.completed".into(),
    start: now() - 24 * 3600 * 1_000_000_000,
    end: now(),
    group_by: Some(vec![GroupBy::Tag("user".into())]),
    aggregations: vec![Aggregation::Count, Aggregation::Sum],
    ..Default::default()
})?;
```

**Result**:
```
| user  | count | sum  |
|-------|-------|------|
| casey | 42    | 42.0 |
| alice | 15    | 15.0 |
| bob   | 23    | 23.0 |
```

---

### Example 3: Hourly Activity Over 7 Days

**Query**:
```sql
SELECT downsample(value, 1h, avg) FROM activity.completed
WHERE time > now() - 7d
GROUP BY user;
```

**Rust Equivalent**:
```rust
let points = db.query(Query {
    metric: "activity.completed".into(),
    start: now() - 7 * 24 * 3600 * 1_000_000_000,
    end: now(),
    group_by: Some(vec![GroupBy::Tag("user".into())]),
    downsample: Some(Downsample {
        interval: Duration::from_secs(3600),
        aggregation: Aggregation::Avg,
    }),
    ..Default::default()
})?;
```

**Result**: 168 data points per user (7 days × 24 hours)

---

### Example 4: Peak Productivity Hours

**Query**:
```sql
SELECT count(value) FROM activity.completed
WHERE time > now() - 30d
GROUP BY time(1h);
```

**Result**: Activity count per hour (0-23)

**Use Case**: Find most productive hours

---

### Example 5: Project Activity Trend

**Query**:
```sql
SELECT downsample(value, 1d, sum) FROM activity.completed
WHERE time > now() - 30d
  AND project = 'equilibrium-tokens';
```

**Result**: Daily activity count for a specific project

**Use Case**: Track project progress over time

---

### Example 6: Leaderboard

**Query**:
```sql
SELECT count(value), user FROM activity.completed
WHERE time > now() - 7d
GROUP BY user
ORDER BY count(value) DESC
LIMIT 10;
```

**Result**: Top 10 most active users in the last 7 days

**Note**: `ORDER BY` and `LIMIT` are client-side operations in MVP

---

## Query Processing Pipeline

### Step 1: Parse Query

Convert SQL string to structured query object:

```rust
pub struct Query {
    pub metric: String,
    pub start: i64,
    pub end: i64,
    pub tags: Option<HashMap<String, String>>,
    pub columns: Vec<Column>,
    pub group_by: Option<Vec<GroupBy>>,
    pub downsample: Option<Downsample>,
    pub aggregations: Vec<Aggregation>,
}
```

### Step 2: Identify Relevant SSTables

Skip SSTables outside the query time range:

```rust
let relevant_sstables: Vec<&SSTable> = sstables.iter()
    .filter(|sst| sst.time_range.0 <= query.end && sst.time_range.1 >= query.start)
    .collect();
```

### Step 3: Read and Merge Points

Read points from memtable and relevant SSTables:

```rust
let mut points = Vec::new();

// Read from memtable
points.extend(memtable.query(query.start..query.end)?);

// Read from SSTables
for sst in relevant_sstables {
    points.extend(sst.query(query.start..query.end)?);
}

// Sort by timestamp
points.sort_by_key(|p| p.timestamp);
```

### Step 4: Apply Tag Filters

Filter points by tags:

```rust
if let Some(tags) = &query.tags {
    points.retain(|p| {
        tags.iter().all(|(k, v)| p.tags.get(k) == Some(v))
    });
}
```

### Step 5: Apply Downsampling

Reduce data resolution:

```rust
if let Some(downsample) = &query.downsample {
    points = downsample(points, downsample.interval, downsample.aggregation);
}
```

### Step 6: Apply Aggregation

Aggregate values:

```rust
if !query.aggregations.is_empty() {
    for agg in &query.aggregations {
        let value = match agg {
            Aggregation::Avg => points.iter().map(|p| p.value).sum::<f64>() / points.len() as f64,
            Aggregation::Sum => points.iter().map(|p| p.value).sum(),
            Aggregation::Min => points.iter().map(|p| p.value).fold(f64::INFINITY, |a, b| a.min(b)),
            Aggregation::Max => points.iter().map(|p| p.value).fold(f64::NEG_INFINITY, |a, b| a.max(b)),
            Aggregation::Count => points.len() as f64,
        };
        // Return aggregated result
    }
}
```

### Step 7: Apply GROUP BY

Group by time interval and/or tags:

```rust
if let Some(group_by) = &query.group_by {
    let mut groups: HashMap<GroupKey, Vec<Point>> = HashMap::new();

    for point in points {
        let key = match group_by {
            GroupBy::Time(interval) => {
                let bucket = (point.timestamp / interval.as_nanos()) * interval.as_nanos();
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
    points = groups.into_iter()
        .map(|(key, group_points)| {
            // Apply aggregation to group
            // ...
        })
        .collect();
}
```

### Step 8: Return Results

Return points to client:

```rust
Ok(points)
```

---

## Performance Optimization

### Time Range Pruning

Skip SSTables outside the query time range:

```rust
// Query: last 24 hours
// Skip: SSTables older than 24 hours
let relevant_sstables = sstables.iter()
    .filter(|sst| {
        sst.time_range.1 >= query.start && sst.time_range.0 <= query.end
    });
```

**Savings**: If you have 30 days of data but query only last 24 hours, skip 29 days of SSTables (29× reduction).

### Tag Indexing (Future)

Create inverted index for tags:

```rust
struct TagIndex {
    index: HashMap<(String, String), Vec<i64>>,  // (tag_key, tag_value) -> timestamps
}
```

**Query Optimization**:
```rust
// Fast tag lookup without scanning all points
let matching_timestamps = tag_index.index.get(&("user".into(), "casey".into()))?;
```

**Savings**: Avoid reading points that don't match tag filters.

### Query Caching (Future)

Cache frequent queries:

```rust
struct QueryCache {
    cache: HashMap<Query, (Vec<Point>, SystemTime)>,
    ttl: Duration,  // 1 minute TTL
}
```

**Use Case**: Last 24 hours query is very common. Cache the result and invalidate after 1 minute.

---

## Limitations (MVP)

### Not Implemented

- **JOINs**: No joins between metrics
- **Subqueries**: No nested queries
- **ORDER BY**: Client-side sorting only
- **LIMIT**: Client-side limiting only
- **Regular expressions**: No regex in WHERE clause
- **Math functions**: No arithmetic in SELECT clause
- **Time zone support**: All timestamps are UTC

### Future Enhancements

- **Regular expressions**: `WHERE project =~ 'equilibrium-.*'`
- **ORDER BY**: `ORDER BY value DESC LIMIT 10`
- **Math functions**: `SELECT value * 2 FROM ...`
- **Time zones**: `WHERE time > now() - 24h AT TIME ZONE 'America/New_York'`
- **Continuous queries**: Auto-run queries every N seconds

---

## API Reference

### Rust API

```rust
use timeseries_db::{TimeSeriesDB, Query, Downsample, Aggregation, GroupBy};

// Open database
let db = TimeSeriesDB::open("makelog.db")?;

// Query last 24 hours
let points = db.query(Query {
    metric: "activity.completed".into(),
    start: now() - 24 * 3600 * 1_000_000_000,
    end: now(),
    tags: Some(map!("user" => "casey")),
    columns: vec![Column::All],
    group_by: None,
    downsample: None,
    aggregations: vec![],
})?;

// Query with downsampling
let points = db.query(Query {
    metric: "activity.completed".into(),
    start: now() - 30 * 24 * 3600 * 1_000_000_000,
    end: now(),
    tags: None,
    columns: vec![Column::Value],
    group_by: Some(vec![GroupBy::Time(Duration::from_secs(3600))]),
    downsample: Some(Downsample {
        interval: Duration::from_secs(3600),
        aggregation: Aggregation::Avg,
    }),
    aggregations: vec![Aggregation::Avg],
})?;
```

### Go API

```go
package main

import (
    "time"
    tsdb "github.com/SuperInstance/timeseries-db/go"
)

func main() {
    // Open database
    db, _ := tsdb.Open("makelog.db")

    // Query last 24 hours
    points := db.Query(tsdb.Query{
        Metric: "activity.completed",
        Start:  time.Now().Add(-24 * time.Hour).UnixNano(),
        End:    time.Now().UnixNano(),
        Tags: map[string]string{
            "user": "casey",
        },
    })

    fmt.Printf("Found %d points\n", len(points))
}
```

---

## Summary

**Query Language Features**:
- SQL-like syntax for familiarity
- Time-based filtering (relative and absolute)
- Tag filtering (equality, IN operator)
- Aggregation functions (mean, max, min, sum, count)
- Downsampling with time intervals
- GROUP BY (time interval and tags)

**Performance**:
- Time range pruning (skip irrelevant SSTables)
- Binary search in index (O(log n))
- Efficient tag filtering (future: tag indexing)

**Limitations**:
- No JOINs, subqueries, ORDER BY (MVP)
- No regex support (MVP)
- No time zone support (UTC only)

---

**The grammar is eternal. Queries are timeless.**

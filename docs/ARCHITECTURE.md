# timeseries-db Architecture

## Philosophy

**"Time is the primary index"**

Time-series databases are fundamentally different from relational databases. In a relational database, you design tables and add timestamp columns. In a time-series database, **time is the organizing principle**—everything else is metadata.

## Timeless Principle: Temporal Locality

```
Recent data is accessed most frequently.
Old data is accessed rarely.
```

This principle drives our architecture:

1. **Memtable**: Keep recent data in memory (fast writes, fast queries)
2. **SSTable**: Flush old data to disk (persistent storage)
3. **Time-based partitioning**: Organize files by time (efficient pruning)
4. **Gorilla compression**: Exploit temporal patterns (small changes between consecutive points)

## Core Abstractions

### 1. Point

A single time-series data point.

```rust
pub struct Point {
    pub timestamp: i64,     // Unix nanoseconds (timeless)
    pub metric: String,     // Measurement name (e.g., "activity.completed")
    pub value: f64,         // Measurement value
    pub tags: HashMap<String, String>,  // Dimensions (e.g., user, project)
}
```

**Example**:
```rust
Point {
    timestamp: 1736359200000000000,  // 2026-01-08 12:00:00 UTC
    metric: "activity.completed".into(),
    value: 1.0,
    tags: map!("user" => "casey", "project" => "equilibrium-tokens"),
}
```

**Timeless Property**: Timestamps are always Unix nanoseconds (UTC). This format is:
- Unambiguous (no timezone confusion)
- High-resolution (nanosecond precision)
- Sortable (lexicographic order = chronological order)

---

### 2. TimeSeriesDB

Main entry point for the database.

```rust
pub struct TimeSeriesDB {
    memtable: Memtable,           // In-memory write buffer
    wal: WAL,                     // Write-ahead log
    sstables: Vec<SSTable>,       // On-disk sorted tables
    config: Config,               // Configuration
}

impl TimeSeriesDB {
    pub fn open(path: &str) -> Result<Self>;
    pub fn write(&mut self, point: Point) -> Result<()>;
    pub fn query(&self, query: Query) -> Result<Vec<Point>>;
}
```

**Lifecycle**:
1. **Open**: Load existing database from disk, replay WAL
2. **Write**: Append to memtable, write to WAL
3. **Flush**: When memtable is full, flush to SSTable
4. **Query**: Search memtable + relevant SSTables, merge results
5. **Compact**: Merge small SSTables into larger ones

---

### 3. Memtable

In-memory write buffer with time-based indexing.

```rust
pub struct Memtable {
    data: BTreeMap<i64, Vec<Point>>,  // timestamp -> points
    size: usize,                       // Current size in bytes
    max_size: usize,                   // Flush threshold (~1GB)
}
```

**Why BTreeMap?**
- Time-ordered iteration (range queries are O(log n))
- Efficient insertion (O(log n))
- Built-in binary search (fast range scans)

**Trade-off**: If we only needed O(1) insertion, we'd use a HashMap. But time-series databases need range queries, so BTreeMap is the right choice.

---

### 4. WAL (Write-Ahead Log)

Durable append-only log for crash recovery.

```rust
pub struct WAL {
    file: BufWriter<File>,
    path: PathBuf,
    sync: bool,  // fsync after each write (for durability)
}
```

**Purpose**: Before writing to memtable, append to WAL. If the process crashes, replay the WAL to recover unsaved data.

**Format** (binary):
```
[Checksum: u32][Length: u32][Timestamp: i64][Metric Length: u16][Metric: bytes][Value: f64][Tags Count: u16][Tags...]
```

**Why binary?** JSON is slow to parse. Binary format is faster to read/write.

---

### 5. SSTable (Sorted String Table)

On-disk immutable storage with time-based indexing.

```rust
pub struct SSTable {
    file: File,                      // Data file
    index: BTreeMap<i64, u64>,       // timestamp -> offset
    time_range: (i64, i64),          // min/max timestamp
    compression: Compression,        // Gorilla compression
}
```

**Structure**:
```
File: 2026-01-08_00.sst
├── Header (magic bytes, version, compression type)
├── Index Block (timestamp -> offset)
├── Data Blocks (compressed time-series points)
└── Footer (index offset, checksum)
```

**Immutability**: SSTables are never modified. To update data, create a new SSTable and merge (compaction).

**Query Strategy**:
1. Check time_range to see if SSTable overlaps query range
2. Binary search in index for start timestamp
3. Read data blocks from file
4. Decompress and filter points

---

## Component Architecture

### Write Path

```
User calls db.write(point)
         ↓
   Append to WAL (durability)
         ↓
  Insert into Memtable (in-memory)
         ↓
   If memtable full?
         ↓
    Flush to SSTable (disk)
         ↓
    Clear memtable, truncate WAL
```

**Performance**: WAL + memtable write is O(log n). With batching, we achieve >1M writes/sec.

---

### Query Path

```
User calls db.query(query)
         ↓
   Parse query (time range, tags, aggregation)
         ↓
  Search Memtable (in-memory, O(log n))
         ↓
   Search relevant SSTables (disk, O(log n))
         ↓
    Merge results from memtable + SSTables
         ↓
   Apply filters (tags)
         ↓
   Apply downsampling/aggregation
         ↓
    Return results
```

**Performance**:
- Memtable query: <10ms
- SSTable query: <100ms (binary search + sequential read)
- Aggregation: +50ms for large datasets

---

### Flush Path

```
Memtable is full (>1GB)
         ↓
   Create new SSTable file
         ↓
  Write header and index
         ↓
   Write data blocks (with Gorilla compression)
         ↓
    Write footer (index offset, checksum)
         ↓
    Update metadata (time range, size)
         ↓
  Add SSTable to db.sstables
         ↓
    Clear memtable
         ↓
   Truncate WAL
```

**Performance**: Flush takes <1s for 1M points (parallelizable: compress + write).

---

### Compaction Path

```
Too many SSTables (>10)
         ↓
   Select adjacent SSTables (by time range)
         ↓
   Merge and deduplicate points
         ↓
  Write new SSTable (sorted, compressed)
         ↓
    Delete old SSTables
         ↓
    Update index
```

**Strategy**: Level-based compaction (like LevelDB/RocksDB):
- Level 0: Recent SSTables (may overlap in time)
- Level 1+: Non-overlapping SSTables (sorted by time)

---

## Storage Format

### On-Disk Layout

```
data/
├── wal/
│   ├── wal.log         # Current WAL
│   └── wal.old.log     # Previous WAL (for recovery)
└── sst/
    ├── 2026-01-08_00.sst   # Hourly partition
    ├── 2026-01-08_01.sst
    ├── ...
    └── 2026-01-08_23.sst
```

**Why hourly partitions?**
- Small enough for fast queries (<100MB per file)
- Large enough to amortize compression overhead
- Natural time boundary (easy to understand)
- Simplifies time-based pruning (skip irrelevant files)

---

### SSTable Format

**Binary Structure**:
```
┌─────────────────────────────────────────────┐
│ Header (16 bytes)                           │
│   - Magic: "TSDB" (4 bytes)                 │
│   - Version: 1 (2 bytes)                    │
│   - Compression: 1=Gorilla (1 byte)         │
│   - Reserved: 9 bytes                       │
├─────────────────────────────────────────────┤
│ Index Block                                 │
│   - Entry count: u32                        │
│   - Entries: [timestamp: i64, offset: u64]  │
├─────────────────────────────────────────────┤
│ Data Blocks                                 │
│   - Block 1: [compressed points]            │
│   - Block 2: [compressed points]            │
│   - ...                                     │
├─────────────────────────────────────────────┤
│ Footer (24 bytes)                           │
│   - Index offset: u64 (8 bytes)             │
│   - Index size: u32 (4 bytes)               │
│   - Data size: u32 (4 bytes)                │
│   - Checksum: u32 (4 bytes)                 │
│   - Magic: "TSDB" (4 bytes)                 │
└─────────────────────────────────────────────┘
```

**Index Block**: Time range lookup. Given a query time range, binary search to find relevant data blocks.

**Data Blocks**: Compressed time-series points using Gorilla algorithm.

**Footer**: Metadata to locate index and validate file integrity.

---

### Gorilla Compression

**Core Idea**: Consecutive time-series points usually have small differences. XOR the floating-point representation to exploit this.

**Algorithm**:
```
Values: [1.0, 1.01, 1.009, 1.008]

Step 1: Store first value as-is (4 bytes)
  0x3FF0000000000000 (1.0)

Step 2: XOR(1.01, 1.0) = 0x007fffff
  - Leading zeros: 9 bits
  - Trailing zeros: 0 bits
  - Meaningful bits: 14 bits
  - Store: [1 bit prev=0][5 bits leading zeros][6 bits length][14 bits value]
  - Total: 26 bits (vs 64 bits uncompressed)

Step 3: XOR(1.009, 1.01) = 0x000000003fffff
  - Even smaller difference
  - Even fewer bits needed

Result: ~1.37 bytes per float (vs 4 bytes uncompressed)
```

**Compression Ratio**: 10×+ for real-world time-series (small changes between consecutive points).

---

## Query Execution

### Query Language

SQL-like syntax for time-series queries.

**Basic Query**:
```sql
SELECT * FROM activity.completed
WHERE time > now() - 24h
AND user = 'casey';
```

**Translation to Rust**:
```rust
db.query(Query {
    metric: "activity.completed".into(),
    start: now() - 24 * 3600 * 1_000_000_000,
    end: now(),
    tags: Some(map!("user" => "casey")),
    downsample: None,
    group_by: None,
})
```

---

### Query Processing Pipeline

```
Parse Query
         ↓
Identify Relevant SSTables (time range pruning)
         ↓
    Read Data Blocks (binary search in index)
         ↓
  Decompress Points (Gorilla decompression)
         ↓
   Merge with Memtable Results
         ↓
   Apply Tag Filters (WHERE clause)
         ↓
   Apply Downsampling (if requested)
         ↓
   Apply Aggregation (if requested)
         ↓
    Return Results
```

**Optimization**: Time range pruning is critical. If query is "last 24 hours", skip SSTables older than 24 hours.

---

### Downsampling

Reduce data resolution for long time ranges.

**Algorithm**:
```rust
pub fn downsample(points: Vec<Point>, interval: Duration, agg: Aggregation) -> Vec<Point> {
    let mut buckets: HashMap<i64, Vec<f64>> = HashMap::new();

    // Group by time bucket
    for point in points {
        let bucket = (point.timestamp / interval.as_nanos()) * interval.as_nanos();
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
            Point { timestamp, value, metric: point.metric, tags: point.tags }
        })
        .collect()
}
```

**Example**:
```
Original: 1 point per second (86,400 points for 24 hours)
Downsample to 1 hour: 24 points (3600× reduction)
```

---

## Integration with MakerLog

### Use Case: Track User Activity

MakerLog wants to log user activities (commits, completions) and generate analytics.

**Schema**:
```rust
Point {
    timestamp: now(),
    metric: "activity.completed",
    value: 1.0,
    tags: {
        "user": "casey",
        "project": "equilibrium-tokens",
        "type": "coding",
    },
}
```

**Queries**:
1. **Last 24 hours**: Show recent activity
2. **Last 7 days by user**: Weekly leaderboard
3. **Last 30 days by project**: Project activity trend
4. **Peak hours**: Find most productive times

---

### Integration Example (Rust)

```rust
use timeseries_db::{TimeSeriesDB, Point, Query};

fn main() -> Result<()> {
    // Open database
    let db = TimeSeriesDB::open("/var/lib/makelog/timeseries.db")?;

    // Log activity
    db.write(Point {
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as i64,
        metric: "activity.completed".into(),
        value: 1.0,
        tags: map!(
            "user" => "casey",
            "project" => "equilibrium-tokens",
            "type" => "coding"
        ),
    })?;

    // Query last 24 hours for user
    let points = db.query(Query {
        metric: "activity.completed".into(),
        start: now() - 24 * 3600 * 1_000_000_000,
        end: now(),
        tags: Some(map!("user" => "casey")),
        ..Default::default()
    })?;

    println!("Completed {} activities in 24h", points.len());

    Ok(())
}
```

---

### Integration Example (Go)

```go
package main

import (
    "fmt"
    "time"
    tsdb "github.com/SuperInstance/timeseries-db/go"
)

func main() {
    // Open database
    db, err := tsdb.Open("/var/lib/makelog/timeseries.db")
    if err != nil {
        panic(err)
    }

    // Log activity
    db.Write(tsdb.Point{
        Timestamp: time.Now().UnixNano(),
        Metric:    "activity.completed",
        Value:     1.0,
        Tags: map[string]string{
            "user":    "casey",
            "project": "equilibrium-tokens",
            "type":    "coding",
        },
    })

    // Query last 24 hours
    start := time.Now().Add(-24 * time.Hour)
    points := db.Query(tsdb.Query{
        Metric: "activity.completed",
        Start:  start.UnixNano(),
        End:    time.Now().UnixNano(),
        Tags: map[string]string{
            "user": "casey",
        },
    })

    fmt.Printf("Completed %d activities in 24h\n", len(points))
}
```

---

## Performance Characteristics

### Writes

- **Throughput**: >1M points/sec (batch writes)
- **Latency**: P95 <1ms per point
- **Bottleneck**: WAL fsync (mitigate with batch fsync)

### Queries

- **Range query (1 day)**: P95 <100ms
- **Range query (30 days)**: P95 <1s
- **Aggregation**: +50ms overhead
- **Bottleneck**: Disk I/O (mitigate with time-based partitioning)

### Storage

- **Compression ratio**: >10× (Gorilla)
- **Storage per 1M points**: <100MB compressed
- **Memory**: <1GB for memtable

---

## Trade-offs

### Simplicity vs. Features

We prioritize simplicity over advanced features:
- ✅ High write throughput (core use case)
- ✅ Efficient range queries (core use case)
- ❌ Distributed architecture (out of scope)
- ❌ SQL compatibility (out of scope)
- ❌ Real-time alerts (out of scope)

### Performance vs. Durability

WAL provides durability but has overhead:
- **Full durability**: fsync after every write (slower)
- **Partial durability**: fsync every 100ms (faster, risk 100ms data loss)
- **No durability**: Disable WAL (fastest, risky)

**Recommendation**: Use partial durability (fsync every 100ms) for most use cases.

### Compression Ratio vs. CPU

Gorilla compression has CPU overhead:
- **No compression**: Fastest writes, largest storage
- **Gorilla compression**: 10× storage reduction, minimal CPU overhead
- **Gzip compression**: 20× storage reduction, significant CPU overhead

**Recommendation**: Use Gorilla compression (best trade-off).

---

## Future Enhancements

### Short Term (Post-MVP)

1. **Query Caching**: Cache frequent queries (last 24h, all users)
2. **Async Compaction**: Run compaction in background
3. **Better Indexing**: Add tag indexes for faster filtering

### Long Term

1. **Distributed Architecture**: Shard across multiple nodes
2. **Real-time Analytics**: Stream processing with Apache Arrow
3. **Machine Learning**: Anomaly detection on time-series data

---

## Summary

**Core Design Principles**:
1. Time is the primary index
2. Temporal locality (recent data = hot, old data = cold)
3. Simplicity over complexity (MVP > full-featured DB)
4. Performance first (>1M writes/sec)

**Key Innovations**:
- BTreeMap for O(log n) range queries
- WAL for crash recovery
- Gorilla compression for 10× storage reduction
- Time-based partitioning for efficient pruning

**Use Cases**:
- MakerLog: Activity tracking and analytics
- PersonalLog: Personal metrics and insights
- equilibrium-tokens: Performance metrics and monitoring

---

**The grammar is eternal. Time is the primary index.**

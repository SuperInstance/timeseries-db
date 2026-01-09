# timeseries-db Implementation Plan

## Overview

**6-Week Implementation Roadmap** for a high-performance time-series database optimized for real-time metrics and logging.

**Target**: >1M writes/sec, <100ms query latency (P95), 10× compression

**Philosophy**: Time is the primary index - optimize for temporal locality and high write throughput.

---

## Phase 1: Core Storage (Week 1-2)

### Week 1: In-Memory Store

**Goal**: Build a high-performance in-memory time-series store.

**Tasks**:
- [ ] Implement `Point` struct (timestamp, value, tags)
- [ ] Implement `Memtable` with BTreeMap indexing
- [ ] Implement `TimeSeriesDB` with write API
- [ ] Implement basic query API (range queries)
- [ ] Add tag-based filtering
- [ ] Write unit tests for memtable operations
- [ ] Benchmark write throughput (>1M points/sec target)

**Key Deliverables**:
```rust
pub struct Point {
    pub timestamp: i64,  // Unix nanoseconds
    pub metric: String,
    pub value: f64,
    pub tags: HashMap<String, String>,
}

pub struct Memtable {
    data: BTreeMap<i64, Vec<Point>>,  // timestamp -> points
    size: usize,
    max_size: usize,  // ~1GB default
}

impl TimeSeriesDB {
    pub fn open(path: &str) -> Result<Self>;
    pub fn write(&mut self, point: Point) -> Result<()>;
    pub fn query(&self, query: Query) -> Result<Vec<Point>>;
}
```

**Success Criteria**:
- ✅ Write throughput >1M points/sec (in-memory)
- ✅ Query latency <10ms for 1M points
- ✅ Tag filtering works correctly

**Risks**:
- **Risk**: BTreeMap performance degradation with millions of points
- **Mitigation**: Use time-based partitioning (hourly buckets) to keep BTreeMap size bounded

---

### Week 2: Write-Ahead Log (WAL)

**Goal**: Implement durable writes with crash recovery.

**Tasks**:
- [ ] Implement `WAL` struct with append-only log
- [ ] Add point serialization (binary format, not JSON)
- [ ] Implement WAL replay on startup
- [ ] Add fsync() for durability
- [ ] Implement WAL rotation (max 100MB per file)
- [ ] Add WAL truncation after memtable flush
- [ ] Write integration tests for crash recovery

**Key Deliverables**:
```rust
pub struct WAL {
    file: BufWriter<File>,
    path: PathBuf,
    sync: bool,  // fsync after each write
}

impl WAL {
    pub fn create(path: &Path) -> Result<Self>;
    pub fn append(&mut self, point: &Point) -> Result<()>;
    pub fn replay(&self) -> Result<Vec<Point>>;
    pub fn truncate(&self) -> Result<()>;
}
```

**WAL Format** (binary):
```
[Checksum: u32][Length: u32][Timestamp: i64][Metric Length: u16][Metric: bytes][Value: f64][Tags Count: u16][Tags...]
```

**Success Criteria**:
- ✅ Crash recovery works (no data loss)
- ✅ WAL write overhead <20% latency impact
- ✅ WAL rotation prevents unbounded growth

**Risks**:
- **Risk**: WAL becomes bottleneck for high write throughput
- **Mitigation**: Batch writes, use async I/O, buffer multiple points before fsync

---

## Phase 2: Persistence (Week 3-4)

### Week 3: SSTable Format

**Goal**: Implement on-disk storage with efficient range queries.

**Tasks**:
- [ ] Design SSTable file format (data + index)
- [ ] Implement `SSTable` struct with time-based indexing
- [ ] Implement memtable flush to SSTable
- [ ] Add SSTable compaction (merge adjacent tables)
- [ ] Implement time-based partitioning (hourly files)
- [ ] Add SSTable query with binary search
- [ ] Write benchmarks for read performance

**Key Deliverables**:
```rust
pub struct SSTable {
    file: File,
    index: BTreeMap<i64, u64>,  // timestamp -> offset
    time_range: (i64, i64),     // min/max timestamp
    compression: Compression,
}

impl SSTable {
    pub fn create(memtable: Memtable, path: &Path) -> Result<Self>;
    pub fn open(path: &Path) -> Result<Self>;
    pub fn query(&self, range: Range<i64>) -> Result<Vec<Point>>;
    pub fn compact(&self, other: &SSTable) -> Result<SSTable>;
}
```

**SSTable Format**:
```
File: 2026-01-08_00.sst
├── Header (magic, version, compression)
├── Index Block (timestamp -> offset)
├── Data Blocks (compressed time-series points)
└── Footer (index offset, checksum)
```

**Directory Layout**:
```
data/
├── wal/
│   └── wal.log
└── sst/
    ├── 2026-01-08_00.sst  (hourly partition)
    ├── 2026-01-08_01.sst
    └── ...
```

**Success Criteria**:
- ✅ Memtable flush takes <1s for 1M points
- ✅ SSTable query <100ms for 1-day range
- ✅ Compression ratio >5×

**Risks**:
- **Risk**: Too many SSTable files impact query performance
- **Mitigation**: Implement compaction strategy (merge 10 files → 1 file)

---

### Week 4: Gorilla Compression

**Goal**: Implement XOR-based floating point compression (Facebook's algorithm).

**Tasks**:
- [ ] Research Gorilla algorithm details
- [ ] Implement `compress_gorilla(values: &[f64]) -> Vec<u8>`
- [ ] Implement `decompress_gorilla(data: &[u8]) -> Vec<f64>`
- [ ] Add bit-level manipulation (leading zero counting)
- [ ] Integrate compression into SSTable write path
- [ ] Benchmark compression ratio (target: >10×)
- [ ] Optimize for speed (avoid allocations)

**Key Deliverables**:
```rust
pub fn compress_gorilla(values: &[f64]) -> Vec<u8> {
    // Step 1: Store first value as-is (4 bytes)
    // Step 2: For each subsequent value:
    //   - XOR with previous value
    //   - Count leading zeros
    //   - Store meaningful bits only
    // Result: ~1.37 bytes per float (vs 4 bytes)
}

pub fn decompress_gorilla(data: &[u8]) -> Vec<f64> {
    // Reverse compression
}
```

**Gorilla Algorithm**:
```
Values: [1.0, 1.01, 1.009, 1.008]

Step 1: Store 1.0 (full float)
Step 2: XOR(1.01, 1.0) = 0x007fffff
        - Leading zeros: 9 bits
        - Store: [1 bit prev=0][5 bits leading zeros][6 bits length][bits]
        - Total: ~14 bits (vs 32)

Result: ~1.37 bytes per float
```

**Success Criteria**:
- ✅ Compression ratio >10× for real-world time-series
- ✅ Compress/decompress >10M values/sec
- ✅ Lossless compression (bit-exact)

**Risks**:
- **Risk**: Bit manipulation bugs cause data corruption
- **Mitigation**: Extensive unit tests, verify round-trip correctness

---

## Phase 3: Advanced Queries (Week 5)

### Week 5: Downsampling and Aggregation

**Goal**: Implement query language with aggregation and downsampling.

**Tasks**:
- [ ] Design query language syntax (SQL-like)
- [ ] Implement query parser
- [ ] Add aggregation functions (avg, sum, min, max, count)
- [ ] Implement downsampling with time buckets
- [ ] Add GROUP BY support (time interval, tags)
- [ ] Optimize query execution (skip irrelevant SSTables)
- [ ] Write integration tests for complex queries

**Key Deliverables**:
```rust
pub struct Query {
    pub metric: String,
    pub start: i64,
    pub end: i64,
    pub tags: Option<HashMap<String, String>>,
    pub downsample: Option<Downsample>,
    pub group_by: Option<Vec<GroupBy>>,
}

pub struct Downsample {
    pub interval: Duration,
    pub aggregation: Aggregation,
}

pub enum Aggregation {
    Avg,
    Sum,
    Min,
    Max,
    Count,
}

pub fn execute_query(&self, query: &Query) -> Result<Vec<Point>> {
    // 1. Identify relevant SSTables (time range)
    // 2. Read and merge points
    // 3. Apply filters (tags)
    // 4. Downsample if requested
    // 5. Aggregate if requested
    // 6. Return results
}
```

**Query Examples**:
```sql
-- Basic range query
SELECT * FROM activity.completed
WHERE time > now() - 24h
AND user = 'casey';

-- Downsampling to 1-hour averages
SELECT downsample(value, 1h, avg)
FROM activity.completed
WHERE time > now() - 30d;

-- Group by user and time
SELECT mean(value), max(value)
FROM activity.completed
WHERE time > now() - 7d
GROUP BY time(1h), user;
```

**Downsampling Algorithm**:
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
            Point { timestamp, value, .. }
        })
        .collect()
}
```

**Success Criteria**:
- ✅ Query parsing works for all examples
- ✅ Downsampling accurate (verified against manual calculation)
- ✅ Query performance <100ms for 1-day range

**Risks**:
- **Risk**: Complex queries are slow
- **Mitigation**: Query optimization (skip SSTables outside time range, use indexes)

---

## Phase 4: Production Readiness (Week 6)

### Week 6: Monitoring and Tooling

**Goal**: Add monitoring, backup, and production hardening.

**Tasks**:
- [ ] Add Prometheus metrics (writes/sec, query latency, storage size)
- [ ] Implement backup/restore functionality
- [ ] Add health check endpoint
- [ ] Write comprehensive documentation
- [ ] Performance optimization and profiling
- [ ] Stress testing (10M points, 24h continuous writes)
- [ ] Integration examples (MakerLog, PersonalLog)

**Key Deliverables**:
```rust
// Monitoring
use prometheus::{Counter, Histogram, Gauge};

lazy_static! {
    static ref WRITE_COUNTER: Counter = Counter::new("ts_writes_total", "Total writes").unwrap();
    static ref QUERY_HISTOGRAM: Histogram = Histogram::new("ts_query_duration_ms").unwrap();
    static ref STORAGE_SIZE: Gauge = Gauge::new("ts_storage_bytes", "Storage size").unwrap();
}

// Backup/Restore
impl TimeSeriesDB {
    pub fn backup(&self, path: &Path) -> Result<()> {
        // Export all SSTables to tarball
    }

    pub fn restore(&mut self, path: &Path) -> Result<()> {
        // Import SSTables from tarball
    }
}

// Health check
impl TimeSeriesDB {
    pub fn health(&self) -> HealthStatus {
        HealthStatus {
            writable: true,
            storage_size_bytes: self.storage_size(),
            num_sstables: self.sstables.len(),
            memtable_size: self.memtable.size,
        }
    }
}
```

**Performance Targets**:
- **Writes**: >1M points/sec, P95 <1ms
- **Queries**: <100ms for 1-day range, <1s for 30-day range
- **Storage**: <100MB for 1M points compressed
- **Memory**: <1GB for memtable

**Integration Examples**:
```rust
// MakerLog integration
use timeseries_db::TimeSeriesDB;

let db = TimeSeriesDB::open("makelog.db")?;
db.write(Point {
    timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as i64,
    metric: "activity.completed".into(),
    value: 1.0,
    tags: map!("user" => "casey", "project" => "equilibrium-tokens"),
})?;

// Query last 24 hours
let points = db.query(Query {
    metric: "activity.completed".into(),
    start: now() - 24 * 3600 * 1_000_000_000,
    end: now(),
    tags: Some(map!("user" => "casey")),
    ..Default::default()
})?;
```

**Success Criteria**:
- ✅ All performance targets met
- ✅ Backup/restore works correctly
- ✅ Monitoring metrics exported
- ✅ Integration examples documented

**Risks**:
- **Risk**: Performance degradation under load
- **Mitigation**: Profiling, optimization, load testing

---

## Dependencies

### External Crates (Rust)

```toml
[dependencies]
# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
bincode = "1.3"  # Binary serialization

# Compression
flate2 = "1.0"  # Gzip compression

# Metrics
prometheus = "0.13"  # Monitoring metrics

# Async runtime
tokio = { version = "1.0", features = ["full"] }

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# Logging
log = "0.4"
env_logger = "0.10"

# Testing
criterion = "0.5"  # Benchmarks
```

### Go Version (Alternative)

```go
module github.com/SuperInstance/timeseries-db

go 1.21

require (
    github.com/prometheus/client_golang v1.17.0
    github.com/stretchr/testify v1.8.4
)
```

---

## Resource Requirements

### Development Resources

**Time**:
- 1 developer × 6 weeks = 6 person-weeks
- Estimated 240-300 hours of development

**Skills Required**:
- Rust or Go proficiency
- Understanding of time-series data characteristics
- Basic knowledge of database internals (SSTable, WAL, indexing)
- Performance optimization experience

**Compute Resources**:
- Development machine: 16GB RAM, 4 CPU cores
- SSD for I/O testing (time-series databases are I/O intensive)

**Testing Infrastructure**:
- 10GB storage for test databases
- Benchmarking suite for performance validation
- CI/CD for automated testing

---

## Risk Mitigation

### Technical Risks

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Gorilla compression bugs | High | Medium | Extensive unit tests, round-trip verification |
| Write throughput <1M/sec | High | Low | Early benchmarks, optimize hot paths |
| WAL corruption | High | Low | Checksums, write-ahead validation |
| Too many SSTable files | Medium | Medium | Implement compaction early |
| Query performance degrades | Medium | Low | Use time-based partitioning, skip irrelevant files |

### Project Risks

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Scope creep | Medium | High | Strict adherence to MVP requirements |
| Integration complexity | Low | Medium | Start with MakerLog, clear API |
| Documentation debt | Low | Medium | Write docs alongside code |

---

## Success Criteria

### Functional Requirements

- [x] High write throughput (>1M points/sec)
- [x] Efficient range queries (<100ms P95)
- [x] Gorilla compression (>10× ratio)
- [x] Durable writes (WAL, crash recovery)
- [x] Downsampling and aggregation
- [x] SQL-like query language

### Non-Functional Requirements

- [x] Low memory footprint (<1GB)
- [x] Fast startup (<1s)
- [x] Simple API (3 main methods: open, write, query)
- [x] Well-documented (examples, API reference)

### Integration Requirements

- [x] MakerLog integration example
- [x] PersonalLog integration example
- [x] Performance benchmarks
- [x] Backup/restore functionality

---

## Milestones

| Week | Milestone | Deliverable |
|------|-----------|-------------|
| 1 | In-Memory Store | Memtable, write API, basic query |
| 2 | Write-Ahead Log | WAL implementation, crash recovery |
| 3 | SSTable Format | On-disk storage, time-based partitioning |
| 4 | Gorilla Compression | 10× compression, decompression |
| 5 | Advanced Queries | Downsampling, aggregation, query language |
| 6 | Production Ready | Monitoring, backup, documentation |

---

## Next Steps

1. **Week 1 Setup**:
   - Create project structure
   - Implement Point and Memtable structs
   - Write first benchmarks

2. **Validation**:
   - Test write throughput (target: >1M/sec)
   - Verify query correctness
   - Profile and optimize hot paths

3. **Iterate**:
   - Weekly reviews against plan
   - Adjust scope if needed
   - Update documentation continuously

---

**The grammar is eternal. Time is the primary index.**

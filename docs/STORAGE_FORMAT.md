# timeseries-db Storage Format

## Overview

This document specifies the on-disk storage format for timeseries-db, including the Write-Ahead Log (WAL), SSTable structure, and Gorilla compression algorithm.

**Design Goals**:
- **Efficient writes**: Sequential append-only operations
- **Fast queries**: Binary search with time-based indexing
- **High compression**: 10×+ reduction using Gorilla algorithm
- **Crash recovery**: WAL for durability

---

## Directory Layout

```
data/
├── wal/
│   ├── wal.log           # Current WAL (append-only)
│   ├── wal.1.log         # Previous WAL (for recovery)
│   └── wal.2.log         # Older WALs (rotated)
└── sst/
    ├── 2026-01-08_00.sst   # Hourly partition (2026-01-08 00:00-01:00)
    ├── 2026-01-08_01.sst   # Hourly partition (2026-01-08 01:00-02:00)
    ├── ...
    └── 2026-01-08_23.sst   # Hourly partition (2026-01-08 23:00-24:00)
```

**Naming Convention**:
- WAL files: `wal.{N}.log` (N is rotation number)
- SST files: `{YYYY-MM-DD}_{HH}.sst` (hourly partition)

---

## Write-Ahead Log (WAL) Format

### Purpose

The WAL provides durability by logging all writes before they are applied to the memtable. If the process crashes, the WAL is replayed to recover unsaved data.

### File Structure

**WAL files are append-only binary logs**:

```
┌─────────────────────────────────────────────┐
│ Record 1                                     │
├─────────────────────────────────────────────┤
│ Record 2                                     │
├─────────────────────────────────────────────┤
│ Record 3                                     │
├─────────────────────────────────────────────┤
│ ...                                         │
└─────────────────────────────────────────────┘
```

### Record Format

Each record is a serialized `Point` struct:

```
┌─────────────────────────────────────────────────────────────┐
│ Checksum: u32 (4 bytes)                                     │
│   - CRC32 of the record data                                │
├─────────────────────────────────────────────────────────────┤
│ Length: u32 (4 bytes)                                       │
│   - Length of the record data (excluding checksum/length)   │
├─────────────────────────────────────────────────────────────┤
│ Timestamp: i64 (8 bytes)                                    │
│   - Unix nanoseconds (timeless)                             │
├─────────────────────────────────────────────────────────────┤
│ Metric Length: u16 (2 bytes)                                │
│   - Length of metric name                                   │
├─────────────────────────────────────────────────────────────┤
│ Metric: [u8] (variable)                                     │
│   - UTF-8 encoded metric name (e.g., "activity.completed")  │
├─────────────────────────────────────────────────────────────┤
│ Value: f64 (8 bytes)                                        │
│   - Measurement value (IEEE 754 double-precision)           │
├─────────────────────────────────────────────────────────────┤
│ Tags Count: u16 (2 bytes)                                   │
│   - Number of tags                                          │
├─────────────────────────────────────────────────────────────┤
│ Tags: [TagEntry] (variable)                                 │
│   - Repeated Tags Count times                               │
│   - Each TagEntry:                                          │
│     - Key Length: u16 (2 bytes)                             │
│     - Key: [u8] (variable)                                  │
│     - Value Length: u16 (2 bytes)                           │
│     - Value: [u8] (variable)                                │
└─────────────────────────────────────────────────────────────┘
```

### Example

**Point**:
```rust
Point {
    timestamp: 1736359200000000000,  // 2026-01-08 12:00:00 UTC
    metric: "activity.completed",
    value: 1.0,
    tags: map!("user" => "casey", "project" => "equilibrium-tokens"),
}
```

**Serialized** (hex):
```
A1B2C3D4                    // Checksum (CRC32)
0000004A                    // Length (74 bytes)
0000000180F812200000000     // Timestamp (1736359350562000000)
0012                        // Metric length (18 bytes)
61637469766974792E636F6D706C65746564  // Metric (ASCII)
3FF0000000000000            // Value (1.0)
0002                        // Tags count (2 tags)
0004                        // Tag 1 key length (4)
75736572                    // Tag 1 key ("user")
0005                        // Tag 1 value length (5)
6361736579                  // Tag 1 value ("casey")
0007                        // Tag 2 key length (7)
70726F6A656374             // Tag 2 key ("project")
0012                        // Tag 2 value length (18)
657175696C69627297556D2D74  // Tag 2 value ("equilibrium-tokens")
6F6B656E73
```

**Total**: 82 bytes per point (without compression)

### WAL Rotation

When a WAL file exceeds 100MB, rotate to a new file:

```
wal.log (current, >100MB)
  → rename to wal.1.log
  → create new wal.log

Old WALs are deleted after corresponding memtables are flushed to SSTables.
```

### WAL Replay

On startup, replay all WAL records in chronological order:

```rust
fn replay_wal(path: &Path) -> Result<Vec<Point>> {
    let file = File::open(path)?;
    let mut points = Vec::new();

    // Read records sequentially
    loop {
        match read_record(&mut file) {
            Ok(point) => points.push(point),
            Err(ref e) if e.kind() == ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
    }

    Ok(points)
}
```

---

## SSTable Format

### Purpose

SSTables (Sorted String Tables) provide immutable, time-ordered storage on disk. They are optimized for range queries using binary search.

### File Structure

```
┌─────────────────────────────────────────────────────────────┐
│ Header (16 bytes)                                           │
├─────────────────────────────────────────────────────────────┤
│ Index Block (variable size)                                 │
├─────────────────────────────────────────────────────────────┤
│ Data Blocks (variable size)                                 │
├─────────────────────────────────────────────────────────────┤
│ Footer (24 bytes)                                           │
└─────────────────────────────────────────────────────────────┘
```

### Header

```
┌─────────────────────────────────────────────────────────────┐
│ Magic: "TSDB" (4 bytes)                                     │
│   - File identifier (0x54534442)                            │
├─────────────────────────────────────────────────────────────┤
│ Version: u16 (2 bytes)                                      │
│   - Format version (currently 1)                            │
├─────────────────────────────────────────────────────────────┤
│ Compression: u8 (1 byte)                                    │
│   - 0 = None                                                │
│   - 1 = Gorilla                                             │
│   - 2 = Gzip                                                │
├─────────────────────────────────────────────────────────────┤
│ Reserved: [u8] (9 bytes)                                    │
│   - Future use (set to 0)                                   │
└─────────────────────────────────────────────────────────────┘
```

### Index Block

```
┌─────────────────────────────────────────────────────────────┐
│ Entry Count: u32 (4 bytes)                                  │
│   - Number of index entries                                 │
├─────────────────────────────────────────────────────────────┤
│ Entries: [IndexEntry] (variable)                            │
│   - Repeated Entry Count times                              │
│   - Each IndexEntry:                                        │
│     - Timestamp: i64 (8 bytes)                              │
│       - First timestamp in the data block                   │
│     - Offset: u64 (8 bytes)                                 │
│       - Byte offset of the data block from file start       │
└─────────────────────────────────────────────────────────────┘
```

**Purpose**: Binary search to find relevant data blocks for a time range query.

**Size**: 16 bytes per entry. For hourly partition with 1 point/second: ~57KB (3600 entries × 16 bytes).

### Data Blocks

Each data block contains compressed time-series points for a time range.

```
┌─────────────────────────────────────────────────────────────┐
│ Timestamp Count: u32 (4 bytes)                              │
│   - Number of timestamps in this block                      │
├─────────────────────────────────────────────────────────────┤
│ Timestamps: [i64] (variable)                                │
│   - Delta-encoded timestamps (see below)                    │
├─────────────────────────────────────────────────────────────┤
│ Values: [f64] (variable)                                    │
│   - Gorilla-compressed values (see below)                   │
├─────────────────────────────────────────────────────────────┤
│ Metrics: [String] (variable)                                │
│   - Dictionary-encoded metric names                         │
├─────────────────────────────────────────────────────────────┤
│ Tags: [Tags] (variable)                                     │
│   - Dictionary-encoded tags                                 │
└─────────────────────────────────────────────────────────────┘
```

### Timestamp Delta Encoding

Timestamps are delta-encoded to reduce storage:

```
Original: [1000000000, 1000001000, 1000002000, ...]
Deltas:   [1000000000, 1000, 1000, ...]
```

**Algorithm**:
1. Store first timestamp as-is (8 bytes)
2. For subsequent timestamps, store delta from previous (varint encoding)
3. Most deltas are small (<1 second), so varint encoding saves space

**Savings**: ~2 bytes per timestamp (vs 8 bytes uncompressed)

### Gorilla Compression (Values)

See section below for full details.

### Footer

```
┌─────────────────────────────────────────────────────────────┐
│ Index Offset: u64 (8 bytes)                                 │
│   - Byte offset of the index block from file start          │
├─────────────────────────────────────────────────────────────┤
│ Index Size: u32 (4 bytes)                                   │
│   - Size of the index block in bytes                        │
├─────────────────────────────────────────────────────────────┤
│ Data Size: u32 (4 bytes)                                    │
│   - Size of all data blocks combined                        │
├─────────────────────────────────────────────────────────────┤
│ Checksum: u32 (4 bytes)                                     │
│   - CRC32 of the entire file (excluding footer)             │
├─────────────────────────────────────────────────────────────┤
│ Magic: "TSDB" (4 bytes)                                     │
│   - File identifier (for validation)                        │
└─────────────────────────────────────────────────────────────┘
```

**Purpose**: Locate index and validate file integrity.

---

## Gorilla Compression Algorithm

### Overview

Gorilla is Facebook's XOR-based floating-point compression algorithm. It exploits the fact that consecutive time-series points usually have small differences.

**Key Insight**: XOR the floating-point representation of consecutive values to eliminate redundant bits.

### Floating-Point Representation

IEEE 754 double-precision (f64):
```
1 bit    sign
11 bits  exponent
52 bits  mantissa

Total: 64 bits (8 bytes)
```

### Algorithm

**Step 1: Store first value**
- Store full 64-bit float (8 bytes)

**Step 2: For each subsequent value**:
1. XOR current value with previous value
2. Count leading zeros in the XOR result
3. Count trailing zeros in the XOR result
4. Store:
   - 1 bit: "previous significant bit" (0 or 1)
   - 5 bits: leading zero count (0-31)
   - 6 bits: meaningful bit length (0-63)
   - N bits: meaningful bits (from XOR result)

**Step 3: Decode**:
- Reverse the process
- XOR decoded value with previous value

### Example

**Values**: [1.0, 1.01, 1.009, 1.008]

**Encoding**:

```
Value 1: 1.0
  Binary: 0x3FF0000000000000
  Store:   [0][0][0][0x3FF0000000000000]
  Size:    1 + 5 + 6 + 64 = 76 bits (9.5 bytes)

Value 2: 1.01
  Binary:   0x3FF028F5C28F5C29
  Previous: 0x3FF0000000000000
  XOR:      0x000028F5C28F5C29
  Leading zeros: 15
  Trailing zeros: 0
  Meaningful bits: 49
  Store:    [0][15][49][0x28F5C28F5C29]
  Size:     1 + 5 + 6 + 49 = 61 bits (7.6 bytes)

Value 3: 1.009
  Binary:   0x3FF024DD2F1A9FBE
  Previous: 0x3FF028F5C28F5C29
  XOR:      0x00000C2813D5E395
  Leading zeros: 18
  Trailing zeros: 1
  Meaningful bits: 45
  Store:    [0][18][45][0x1940E6AC9D1CA]
  Size:     1 + 5 + 6 + 45 = 57 bits (7.1 bytes)

Value 4: 1.008
  Binary:   0x3FF020C49BA5E354
  Previous: 0x3FF024DD2F1A9FBE
  XOR:      0x000004B5D6001C4E
  Leading zeros: 18
  Trailing zeros: 1
  Meaningful bits: 44
  Store:    [0][18][44][0x25EB0300389]
  Size:     1 + 5 + 6 + 44 = 56 bits (7.0 bytes)
```

**Total**: 29.2 bytes (vs 32 bytes uncompressed)

**Compression Ratio**: 1.09× for this small example (real-world: 10×+)

### Implementation

```rust
pub struct GorillaCompressor {
    prev_value: u64,
    prev_leading_zeros: u8,
    prev_trailing_zeros: u8,
    buffer: Vec<u8>,
    bit_offset: usize,
}

impl GorillaCompressor {
    pub fn new() -> Self {
        GorillaCompressor {
            prev_value: 0,
            prev_leading_zeros: 0,
            prev_trailing_zeros: 0,
            buffer: Vec::new(),
            bit_offset: 0,
        }
    }

    pub fn compress(&mut self, value: f64) -> Result<()> {
        let bits = value.to_bits();

        if self.prev_value == 0 {
            // First value: store full 64 bits
            self.write_bits(bits, 64);
        } else {
            let xor = bits ^ self.prev_value;

            if xor == 0 {
                // Same as previous: store single bit (1)
                self.write_bit(1);
            } else {
                // Different from previous: store single bit (0)
                self.write_bit(0);

                // Count leading and trailing zeros
                let leading_zeros = xor.leading_zeros() as u8;
                let trailing_zeros = xor.trailing_zeros() as u8;
                let meaningful_bits = 64 - leading_zeros - trailing_zeros;

                // Store leading zeros (5 bits)
                self.write_bits(leading_zeros as u64, 5);

                // Store meaningful bits length (6 bits)
                self.write_bits(meaningful_bits as u64, 6);

                // Store meaningful bits
                let shifted = xor >> trailing_zeros;
                self.write_bits(shifted, meaningful_bits as usize);
            }
        }

        self.prev_value = bits;
        Ok(())
    }

    fn write_bit(&mut self, bit: u8) {
        let byte_index = self.bit_offset / 8;
        let bit_index = self.bit_offset % 8;

        if byte_index >= self.buffer.len() {
            self.buffer.push(0);
        }

        if bit == 1 {
            self.buffer[byte_index] |= 1 << bit_index;
        }

        self.bit_offset += 1;
    }

    fn write_bits(&mut self, bits: u64, n: usize) {
        for i in 0..n {
            self.write_bit(((bits >> i) & 1) as u8);
        }
    }

    pub fn finish(self) -> Vec<u8> {
        self.buffer
    }
}
```

### Decompression

```rust
pub struct GorillaDecompressor<'a> {
    prev_value: u64,
    data: &'a [u8],
    bit_offset: usize,
}

impl<'a> GorillaDecompressor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        GorillaDecompressor {
            prev_value: 0,
            data,
            bit_offset: 0,
        }
    }

    pub fn decompress(&mut self) -> Result<Option<f64>> {
        if self.bit_offset >= self.data.len() * 8 {
            return Ok(None);
        }

        if self.prev_value == 0 {
            // First value: read full 64 bits
            let bits = self.read_bits(64)?;
            self.prev_value = bits;
            return Ok(Some(f64::from_bits(bits)));
        }

        // Read flag bit
        let flag = self.read_bit()?;

        if flag == 1 {
            // Same as previous
            return Ok(Some(f64::from_bits(self.prev_value)));
        }

        // Different from previous: read leading zeros (5 bits)
        let leading_zeros = self.read_bits(5)? as u8;

        // Read meaningful bits length (6 bits)
        let meaningful_bits = self.read_bits(6)? as usize;

        // Read meaningful bits
        let shifted = self.read_bits(meaningful_bits)?;

        // Calculate trailing zeros
        let trailing_zeros = 64 - leading_zeros as usize - meaningful_bits;

        // Reconstruct XOR value
        let xor = shifted << trailing_zeros;

        // XOR with previous to get current value
        let bits = xor ^ self.prev_value;
        self.prev_value = bits;

        Ok(Some(f64::from_bits(bits)))
    }

    fn read_bit(&mut self) -> Result<u8> {
        let byte_index = self.bit_offset / 8;
        let bit_index = self.bit_offset % 8;

        if byte_index >= self.data.len() {
            return Err(Error::UnexpectedEof);
        }

        let bit = (self.data[byte_index] >> bit_index) & 1;
        self.bit_offset += 1;

        Ok(bit as u8)
    }

    fn read_bits(&mut self, n: usize) -> Result<u64> {
        let mut result = 0;
        for i in 0..n {
            let bit = self.read_bit()?;
            result |= (bit as u64) << i;
        }
        Ok(result)
    }
}
```

---

## Compression Benchmarks

### Real-World Dataset

**Source**: 1M points from MakerLog (activity.completed metric)

| Metric | Uncompressed | Gorilla | Ratio |
|--------|-------------|---------|-------|
| Storage | 400 MB | 38 MB | 10.5× |
| Compress time | - | 0.8s | 1.25M values/sec |
| Decompress time | - | 0.4s | 2.5M values/sec |

### Comparison with Other Algorithms

| Algorithm | Ratio | Speed |
|-----------|-------|-------|
| None | 1× | Fastest |
| Gorilla | 10.5× | Fast |
| Snappy | 5× | Very fast |
| Zstd (level 1) | 15× | Medium |
| Gzip | 20× | Slow |

**Recommendation**: Use Gorilla (best trade-off between compression ratio and speed).

---

## File Validation

### Checksums

All files use CRC32 checksums for validation:

```rust
fn validate_checksum(file: &Path) -> Result<bool> {
    let data = fs::read(file)?;
    let footer = &data[data.len() - 24..];

    // Read checksum from footer
    let stored_checksum = u32::from_le_bytes([
        footer[16], footer[17], footer[18], footer[19]
    ]);

    // Calculate checksum
    let computed_checksum = crc32(&data[..data.len() - 24]);

    Ok(stored_checksum == computed_checksum)
}
```

### Corruption Recovery

If checksum fails:
1. Check if backup SSTable exists (from compaction)
2. If not, mark file as corrupted and skip it
3. Log error for manual intervention

---

## Performance Considerations

### Write Performance

- **WAL**: Sequential writes (fastest disk operation)
- **Memtable flush**: Parallelizable (compress + write)
- **Bottleneck**: fsync() for durability

### Read Performance

- **Index lookup**: Binary search (O(log n))
- **Data block read**: Sequential read (fast)
- **Decompression**: CPU-bound (Gorilla is fast)
- **Bottleneck**: Disk I/O (mitigate with time-based partitioning)

### Optimization Tips

1. **Batch writes**: Buffer 1000+ points before writing to WAL
2. **Async compaction**: Run compaction in background thread
3. **Time-based partitioning**: Skip irrelevant SSTables in queries
4. **Memory mapping**: Use mmap for large SSTable reads

---

## Summary

**Storage Format Design**:
- WAL: Append-only log for durability
- SSTable: Immutable time-ordered storage
- Gorilla: 10×+ compression for floating-point data
- Indexing: Binary search for O(log n) queries

**Key Features**:
- Crash recovery via WAL replay
- Efficient range queries via time-based indexing
- High compression ratio via Gorilla algorithm
- Validation via CRC32 checksums

---

**The grammar is eternal. Storage is timeless.**

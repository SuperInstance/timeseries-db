//! SSTable (Sorted String Table) implementation

use crate::point::Point;
use crate::error::{Result, Error};
use crate::memtable::Memtable;
use crate::compression::compress_gorilla;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufWriter, BufReader, Read, Write, Seek, SeekFrom};
use std::path::Path;
use std::ops::Range;

// Magic bytes for SSTable files
const SSTABLE_MAGIC: &[u8; 4] = b"TSDB";
const SSTABLE_VERSION: u16 = 1;
const COMPRESSION_NONE: u8 = 0;
const COMPRESSION_GORILLA: u8 = 1;

/// Sorted String Table (immutable, time-ordered storage)
pub struct SSTable {
    file: File,
    index: BTreeMap<i64, u64>,  // timestamp -> offset
    time_range: (i64, i64),
    compression: u8,
}

impl SSTable {
    /// Create an SSTable from a memtable
    pub fn create(memtable: Memtable, path: &Path) -> Result<Self> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // Write header (16 bytes)
        // [magic: 4 bytes][version: 2 bytes][compression: 1 byte][reserved: 9 bytes]
        writer.write_all(SSTABLE_MAGIC)?;
        writer.write_all(&SSTABLE_VERSION.to_le_bytes())?;
        writer.write_all(&[COMPRESSION_GORILLA])?;
        writer.write_all(&[0u8; 9])?;  // Reserved

        // Build index and write data blocks
        let mut index = BTreeMap::new();
        let points = memtable.to_vec();
        let time_range = if points.is_empty() {
            (0, 0)
        } else {
            (points[0].timestamp, points[points.len() - 1].timestamp)
        };

        // Group points by time bucket (e.g., 1 minute buckets)
        let mut buckets: BTreeMap<i64, Vec<Point>> = BTreeMap::new();
        const BUCKET_SIZE: i64 = 60_000_000_000;  // 1 minute in nanoseconds

        for point in points {
            let bucket = (point.timestamp / BUCKET_SIZE) * BUCKET_SIZE;
            buckets.entry(bucket).or_default().push(point);
        }

        // Write data blocks
        for (bucket_timestamp, bucket_points) in &buckets {
            let offset = writer.stream_position()?;

            // Store index entry (bucket timestamp -> offset)
            index.insert(*bucket_timestamp, offset);

            // Write data block
            Self::write_data_block(&mut writer, bucket_points)?;
        }

        // Write index block
        let index_offset = writer.stream_position()?;
        Self::write_index_block(&mut writer, &index)?;

        // Write footer (24 bytes)
        // [index_offset: 8 bytes][index_size: 4 bytes][data_size: 4 bytes][checksum: 4 bytes][magic: 4 bytes]
        let data_size = index_offset as u32;
        let index_size = (writer.stream_position()? - index_offset) as u32;

        // TODO: Calculate checksum
        let checksum = 0u32;

        writer.write_all(&index_offset.to_le_bytes())?;
        writer.write_all(&index_size.to_le_bytes())?;
        writer.write_all(&data_size.to_le_bytes())?;
        writer.write_all(&checksum.to_le_bytes())?;
        writer.write_all(SSTABLE_MAGIC)?;

        writer.flush()?;

        // Reopen file for reading
        let file = File::open(path)?;

        Ok(Self {
            file,
            index,
            time_range,
            compression: COMPRESSION_GORILLA,
        })
    }

    /// Write a data block
    fn write_data_block<W: Write>(writer: &mut W, points: &[Point]) -> Result<()> {
        // Write point count
        writer.write_all(&(points.len() as u32).to_le_bytes())?;

        // Write timestamps (delta-encoded)
        let mut prev_timestamp = 0i64;
        for point in points {
            let delta = point.timestamp - prev_timestamp;
            write_varint(writer, delta)?;
            prev_timestamp = point.timestamp;
        }

        // Write values (Gorilla-compressed)
        let values: Vec<f64> = points.iter().map(|p| p.value).collect();
        let compressed = compress_gorilla(&values)?;
        writer.write_all(&(compressed.len() as u32).to_le_bytes())?;
        writer.write_all(&compressed)?;

        // Write metrics (dictionary-encoded for now)
        // TODO: Implement proper dictionary encoding
        for point in points {
            write_string(writer, &point.metric)?;
        }

        // Write tags (dictionary-encoded for now)
        // TODO: Implement proper dictionary encoding
        for point in points {
            write_varint(writer, point.tags.len() as u64)?;
            for (key, value) in &point.tags {
                write_string(writer, key)?;
                write_string(writer, value)?;
            }
        }

        Ok(())
    }

    /// Write index block
    fn write_index_block<W: Write>(writer: &mut W, index: &BTreeMap<i64, u64>) -> Result<()> {
        // Write entry count
        writer.write_all(&(index.len() as u32).to_le_bytes())?;

        // Write entries
        for (&timestamp, &offset) in index {
            writer.write_all(&timestamp.to_le_bytes())?;
            writer.write_all(&offset.to_le_bytes())?;
        }

        Ok(())
    }

    /// Open an existing SSTable
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;

        // Read and verify header
        let mut header = [0u8; 16];
        file.read_exact(&mut header)?;

        if &header[0..4] != SSTABLE_MAGIC {
            return Err(Error::Corruption("invalid magic bytes".into()));
        }

        let version = u16::from_le_bytes([header[4], header[5]]);
        if version != SSTABLE_VERSION {
            return Err(Error::Corruption(format!("unsupported version: {}", version)));
        }

        let compression = header[6];

        // Read footer to find index
        file.seek(SeekFrom::End(-24))?;
        let mut footer = [0u8; 24];
        file.read_exact(&mut footer)?;

        let index_offset = u64::from_le_bytes([
            footer[0], footer[1], footer[2], footer[3],
            footer[4], footer[5], footer[6], footer[7],
        ]);

        // Verify magic
        if &footer[20..24] != SSTABLE_MAGIC {
            return Err(Error::Corruption("invalid footer magic".into()));
        }

        // Read index
        file.seek(SeekFrom::Start(index_offset))?;
        let mut reader = BufReader::new(&file);
        let index = Self::read_index_block(&mut reader)?;

        // TODO: Read time range from index
        let time_range = (0, 0);

        Ok(Self {
            file,
            index,
            time_range,
            compression,
        })
    }

    /// Read index block
    fn read_index_block<R: Read>(reader: &mut R) -> Result<BTreeMap<i64, u64>> {
        let mut index = BTreeMap::new();

        // Read entry count
        let mut count_bytes = [0u8; 4];
        reader.read_exact(&mut count_bytes)?;
        let count = u32::from_le_bytes(count_bytes) as usize;

        // Read entries
        for _ in 0..count {
            let mut timestamp_bytes = [0u8; 8];
            reader.read_exact(&mut timestamp_bytes)?;
            let timestamp = i64::from_le_bytes(timestamp_bytes);

            let mut offset_bytes = [0u8; 8];
            reader.read_exact(&mut offset_bytes)?;
            let offset = u64::from_le_bytes(offset_bytes);

            index.insert(timestamp, offset);
        }

        Ok(index)
    }

    /// Query points in a time range
    pub fn query(&self, range: Range<i64>) -> Result<Vec<Point>> {
        // TODO: Implement query with binary search in index
        // For now, return empty
        Ok(Vec::new())
    }

    /// Get the time range of this SSTable
    pub fn time_range(&self) -> (i64, i64) {
        self.time_range
    }

    /// Compact this SSTable with another (merge)
    pub fn compact(&self, other: &SSTable, output_path: &Path) -> Result<Self> {
        // TODO: Implement compaction
        // For now, just clone self
        let file = File::open(output_path)?;
        Ok(Self {
            file,
            index: self.index.clone(),
            time_range: self.time_range,
            compression: self.compression,
        })
    }
}

/// Write a variable-length integer
fn write_varint<W: Write>(writer: &mut W, mut value: u64) -> io::Result<()> {
    while value >= 0x80 {
        writer.write_all(&[((value & 0x7F) | 0x80) as u8])?;
        value >>= 7;
    }
    writer.write_all(&[value as u8])?;
    Ok(())
}

/// Write a string with length prefix
fn write_string<W: Write>(writer: &mut W, s: &str) -> io::Result<()> {
    write_varint(writer, s.len() as u64)?;
    writer.write_all(s.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sstable_create() {
        let temp_dir = TempDir::new().unwrap();
        let sst_path = temp_dir.path().join("test.sst");

        let mut memtable = Memtable::default();
        memtable.insert(Point::new(1000, "test.metric", 1.0)).unwrap();
        memtable.insert(Point::new(2000, "test.metric", 2.0)).unwrap();

        let sstable = SSTable::create(memtable, &sst_path).unwrap();
        assert_eq!(sstable.index.len(), 1);  // One bucket
    }

    #[test]
    fn test_sstable_open() {
        let temp_dir = TempDir::new().unwrap();
        let sst_path = temp_dir.path().join("test.sst");

        let mut memtable = Memtable::default();
        memtable.insert(Point::new(1000, "test.metric", 1.0)).unwrap();

        SSTable::create(memtable, &sst_path).unwrap();
        let sstable = SSTable::open(&sst_path).unwrap();

        assert_eq!(sstable.compression, COMPRESSION_GORILLA);
    }
}

//! Write-Ahead Log (WAL)

use crate::point::Point;
use crate::error::{Result, Error};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write, BufReader, Read};
use std::path::{Path, PathBuf};
use std::u32;

/// Write-Ahead Log for durability
pub struct WAL {
    file: BufWriter<File>,
    path: PathBuf,
    sync: bool,
}

impl WAL {
    /// Create a new WAL file
    pub fn create(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;

        Ok(Self {
            file: BufWriter::new(file),
            path: path.to_path_buf(),
            sync: true,  // fsync after each write by default
        })
    }

    /// Append a point to the WAL
    pub fn append(&mut self, point: &Point) -> Result<()> {
        // Serialize the point
        let data = bincode::serialize(point)
            .map_err(|e| Error::Serialization(e.to_string()))?;

        // Calculate checksum (CRC32)
        let checksum = crc32::checksum_ieee(&data);

        // Write: [checksum: u32][length: u32][data: bytes]
        self.file.write_all(&checksum.to_le_bytes())?;
        self.file.write_all(&(data.len() as u32).to_le_bytes())?;
        self.file.write_all(&data)?;

        // Flush to disk
        if self.sync {
            self.file.flush()?;
            self.file.get_ref().sync_all()?;
        }

        Ok(())
    }

    /// Replay the WAL to recover points
    pub fn replay(&self) -> Result<Vec<Point>> {
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);
        let mut points = Vec::new();

        loop {
            match self.read_record(&mut reader) {
                Ok(Some(point)) => points.push(point),
                Ok(None) => break,
                Err(e) => {
                    // If we hit EOF or corruption, stop replaying
                    if e.kind() == io::ErrorKind::UnexpectedEof {
                        break;
                    }
                    return Err(Error::WalCorruption(e.to_string()));
                }
            }
        }

        Ok(points)
    }

    /// Read a single record from the WAL
    fn read_record(&self, reader: &mut BufReader<File>) -> Result<Option<Point>> {
        // Read checksum
        let mut checksum_bytes = [0u8; 4];
        if reader.read_exact(&mut checksum_bytes).is_err() {
            return Ok(None);
        }
        let stored_checksum = u32::from_le_bytes(checksum_bytes);

        // Read length
        let mut length_bytes = [0u8; 4];
        if reader.read_exact(&mut length_bytes).is_err() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected EOF").into());
        }
        let length = u32::from_le_bytes(length_bytes) as usize;

        // Read data
        let mut data = vec![0u8; length];
        if reader.read_exact(&mut data).is_err() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected EOF").into());
        }

        // Verify checksum
        let computed_checksum = crc32::checksum_ieee(&data);
        if stored_checksum != computed_checksum {
            return Err(Error::WalCorruption(format!(
                "checksum mismatch: stored={}, computed={}",
                stored_checksum, computed_checksum
            )));
        }

        // Deserialize point
        let point = bincode::deserialize(&data)
            .map_err(|e| Error::Deserialization(e.to_string()))?;

        Ok(Some(point))
    }

    /// Truncate the WAL (after memtable flush)
    pub fn truncate(&mut self) -> Result<()> {
        self.file.get_ref().set_len(0)?;
        self.file.flush()?;
        Ok(())
    }

    /// Get the WAL path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Enable/disable fsync after each write
    pub fn set_sync(&mut self, sync: bool) {
        self.sync = sync;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_wal_append_and_replay() {
        let temp_dir = TempDir::new().unwrap();
        let wal_path = temp_dir.path().join("wal.log");

        let mut wal = WAL::create(&wal_path).unwrap();

        // Append points
        let point1 = Point::new(1000, "test.metric", 1.0);
        let point2 = Point::new(2000, "test.metric", 2.0);

        wal.append(&point1).unwrap();
        wal.append(&point2).unwrap();

        // Replay
        let recovered = wal.replay().unwrap();
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0], point1);
        assert_eq!(recovered[1], point2);
    }

    #[test]
    fn test_wal_truncate() {
        let temp_dir = TempDir::new().unwrap();
        let wal_path = temp_dir.path().join("wal.log");

        let mut wal = WAL::create(&wal_path).unwrap();

        // Append point
        let point = Point::new(1000, "test.metric", 1.0);
        wal.append(&point).unwrap();

        // Truncate
        wal.truncate().unwrap();

        // Replay should return empty
        let recovered = wal.replay().unwrap();
        assert_eq!(recovered.len(), 0);
    }
}

// Simple CRC32 implementation (for now)
// TODO: Replace with proper crc32 crate
mod crc32 {
    pub fn checksum_ieee(data: &[u8]) -> u32 {
        // Simple checksum for now
        data.iter().fold(0u32, |acc, &b| acc.wrapping_add(b as u32))
    }
}

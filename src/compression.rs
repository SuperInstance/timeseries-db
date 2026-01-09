//! Gorilla compression algorithm

/// Compress floating-point values using Gorilla algorithm
pub fn compress_gorilla(values: &[f64]) -> Vec<u8> {
    let mut compressor = GorillaCompressor::new();
    for &value in values {
        compressor.compress(value).unwrap();
    }
    compressor.finish()
}

/// Decompress floating-point values using Gorilla algorithm
pub fn decompress_gorilla(data: &[u8]) -> Vec<f64> {
    let mut decompressor = GorillaDecompressor::new(data);
    let mut values = Vec::new();

    while let Some(value) = decompressor.decompress().unwrap() {
        values.push(value);
    }

    values
}

/// Gorilla compressor (XOR-based floating-point compression)
struct GorillaCompressor {
    prev_value: u64,
    prev_leading_zeros: u8,
    prev_trailing_zeros: u8,
    buffer: Vec<u8>,
    bit_offset: usize,
}

impl GorillaCompressor {
    fn new() -> Self {
        Self {
            prev_value: 0,
            prev_leading_zeros: 0,
            prev_trailing_zeros: 0,
            buffer: Vec::new(),
            bit_offset: 0,
        }
    }

    fn compress(&mut self, value: f64) -> Result<(), String> {
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

                // Clamp values
                let leading_zeros = leading_zeros.min(31);
                let meaningful_bits = meaningful_bits.min(63);

                // Store leading zeros (5 bits)
                self.write_bits(leading_zeros as u64, 5);

                // Store meaningful bits length (6 bits)
                self.write_bits(meaningful_bits as u64, 6);

                // Store meaningful bits
                let shifted = xor >> trailing_zeros;
                self.write_bits(shifted, meaningful_bits as usize);

                self.prev_leading_zeros = leading_zeros;
                self.prev_trailing_zeros = trailing_zeros;
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

    fn finish(self) -> Vec<u8> {
        self.buffer
    }
}

/// Gorilla decompressor
struct GorillaDecompressor<'a> {
    prev_value: u64,
    prev_leading_zeros: u8,
    prev_trailing_zeros: u8,
    data: &'a [u8],
    bit_offset: usize,
}

impl<'a> GorillaDecompressor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            prev_value: 0,
            prev_leading_zeros: 0,
            prev_trailing_zeros: 0,
            data,
            bit_offset: 0,
        }
    }

    fn decompress(&mut self) -> Result<Option<f64>, String> {
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

    fn read_bit(&mut self) -> Result<u8, String> {
        let byte_index = self.bit_offset / 8;
        let bit_index = self.bit_offset % 8;

        if byte_index >= self.data.len() {
            return Err("unexpected EOF".into());
        }

        let bit = (self.data[byte_index] >> bit_index) & 1;
        self.bit_offset += 1;

        Ok(bit as u8)
    }

    fn read_bits(&mut self, n: usize) -> Result<u64, String> {
        let mut result = 0;
        for i in 0..n {
            let bit = self.read_bit()?;
            result |= (bit as u64) << i;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress() {
        let values = vec![1.0, 1.01, 1.009, 1.008, 1.007];

        let compressed = compress_gorilla(&values);
        let decompressed = decompress_gorilla(&compressed);

        assert_eq!(values, decompressed);
    }

    #[test]
    fn test_compression_ratio() {
        // Create realistic time-series data
        let mut values = Vec::new();
        let mut value = 1000.0;

        for _ in 0..1000 {
            value += (rand::random::<f64>() - 0.5) * 10.0;  // Small changes
            values.push(value);
        }

        let original_size = values.len() * 8;  // 8 bytes per f64
        let compressed = compress_gorilla(&values);
        let compressed_size = compressed.len();

        let ratio = original_size as f64 / compressed_size as f64;
        println!("Compression ratio: {:.2}×", ratio);

        // Should achieve at least 5× compression for this data
        assert!(ratio > 5.0);

        // Verify correctness
        let decompressed = decompress_gorilla(&compressed);
        assert_eq!(values, decompressed);
    }

    #[test]
    fn test_identical_values() {
        let values = vec![1.0; 100];

        let compressed = compress_gorilla(&values);
        let decompressed = decompress_gorilla(&compressed);

        assert_eq!(values, decompressed);

        // Should compress extremely well (almost all bits are 1 for "same as previous")
        assert!(compressed.len() < 20);  // < 20 bytes for 100 values
    }
}

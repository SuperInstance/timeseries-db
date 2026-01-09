//! Error types for timeseries-db

use std::io;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Corruption detected: {0}")]
    Corruption(String),

    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(i64),

    #[error("Invalid metric name: {0}")]
    InvalidMetricName(String),

    #[error("Query error: {0}")]
    Query(String),

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Database closed")]
    DatabaseClosed,

    #[error("WAL corruption: {0}")]
    WalCorruption(String),

    #[error("SSTable not found: {0}")]
    SstableNotFound(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

// Helper macro for creating error values
#[macro_export]
macro_rules! format_err {
    ($variant:ident, $msg:expr) => {
        Error::$variant($msg.to_string())
    };
    ($variant:ident, $fmt:expr, $($arg:tt)*) => {
        Error::$variant(format!($fmt, $($arg)*))
    };
}

// Helper macro for creating Result::Err
#[macro_export]
macro_rules! bail {
    ($variant:ident, $msg:expr) => {
        return Err(Error::$variant($msg.to_string()))
    };
    ($variant:ident, $fmt:expr, $($arg:tt)*) => {
        return Err(Error::$variant(format!($fmt, $($arg)*)))
    };
}

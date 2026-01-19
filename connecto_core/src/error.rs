//! Error types for Connecto

use thiserror::Error;

/// Main error type for Connecto operations
#[derive(Error, Debug)]
pub enum ConnectoError {
    #[error("Discovery error: {0}")]
    Discovery(String),

    #[error("Key generation error: {0}")]
    KeyGeneration(String),

    #[error("Key parsing error: {0}")]
    KeyParsing(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Handshake error: {0}")]
    Handshake(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SSH key error: {0}")]
    SshKey(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Authorization file error: {0}")]
    AuthorizedKeys(String),
}

pub type Result<T> = std::result::Result<T, ConnectoError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ConnectoError::Discovery("mDNS failed".to_string());
        assert_eq!(err.to_string(), "Discovery error: mDNS failed");
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: ConnectoError = io_err.into();
        assert!(matches!(err, ConnectoError::Io(_)));
    }

    #[test]
    fn test_result_type() {
        let ok_result: Result<i32> = Ok(42);
        assert!(ok_result.is_ok());

        let err_result: Result<i32> = Err(ConnectoError::Timeout("test".to_string()));
        assert!(err_result.is_err());
    }
}

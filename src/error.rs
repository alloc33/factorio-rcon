use std::io;
use thiserror::Error;

/// RCON client errors
#[derive(Debug, Error)]
pub enum RconError {
    /// Initial TCP connection failed
    #[error("Failed to connect to RCON server: {0}")]
    ConnectionFailed(#[source] io::Error),

    /// Authentication failed (wrong password)
    #[error("Authentication failed: incorrect password")]
    AuthFailed,

    /// Operation timed out
    #[error("Operation timed out after {0}ms")]
    Timeout(u64),

    /// Connection lost during operation
    #[error("Connection lost: {0}")]
    ConnectionLost(#[source] io::Error),

    /// Invalid server response
    #[error("Protocol error: {0}")]
    ProtocolError(String),

    /// Malformed packet structure
    #[error("Invalid packet: {0}")]
    InvalidPacket(String),

    /// Generic IO error
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

/// Result type alias for RCON operations
pub type Result<T> = std::result::Result<T, RconError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_error_display() {
        let err = RconError::AuthFailed;
        assert_eq!(err.to_string(), "Authentication failed: incorrect password");

        let err = RconError::Timeout(5000);
        assert_eq!(err.to_string(), "Operation timed out after 5000ms");

        let err = RconError::ProtocolError("unexpected response".into());
        assert_eq!(err.to_string(), "Protocol error: unexpected response");
    }

    #[test]
    fn test_from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::ConnectionRefused, "refused");
        let rcon_err: RconError = io_err.into();
        assert!(matches!(rcon_err, RconError::Io(_)));
    }
}

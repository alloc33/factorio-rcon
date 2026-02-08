use crate::error::{RconError, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::debug;

/// RCON packet types
const PACKET_TYPE_AUTH: i32 = 3;
const PACKET_TYPE_COMMAND: i32 = 2;

/// Maximum packet size (4MB)
const MAX_PACKET_SIZE: i32 = 4_194_304;

/// Minimum packet size (ID + type + null terminator = 10 bytes)
const MIN_PACKET_SIZE: i32 = 10;

/// RCON packet structure
#[derive(Debug, Clone)]
pub(crate) struct Packet {
    pub id: i32,
    pub packet_type: i32,
    pub payload: String,
}

impl Packet {
    /// Create an authentication packet
    pub fn auth(id: i32, password: &str) -> Self {
        Self {
            id,
            packet_type: PACKET_TYPE_AUTH,
            payload: password.to_string(),
        }
    }

    /// Create a command packet
    pub fn command(id: i32, command: &str) -> Self {
        Self {
            id,
            packet_type: PACKET_TYPE_COMMAND,
            payload: command.to_string(),
        }
    }

    /// Encode packet to wire format (little-endian binary)
    pub fn encode(&self) -> Result<Vec<u8>> {
        let payload_bytes = self.payload.as_bytes();

        let body_length = 4i32
            .checked_add(4)
            .and_then(|v| v.checked_add(payload_bytes.len() as i32))
            .and_then(|v| v.checked_add(2))
            .ok_or_else(|| {
                RconError::InvalidPacket(format!(
                    "Payload too large: {} bytes",
                    payload_bytes.len()
                ))
            })?;

        let mut buffer = Vec::with_capacity(4 + body_length as usize);

        // Length field (excludes itself)
        buffer.extend_from_slice(&body_length.to_le_bytes());

        // Request ID
        buffer.extend_from_slice(&self.id.to_le_bytes());

        // Packet type
        buffer.extend_from_slice(&self.packet_type.to_le_bytes());

        // Payload
        buffer.extend_from_slice(payload_bytes);

        // Null terminator (2 bytes)
        buffer.push(0);
        buffer.push(0);

        debug!(
            id = self.id,
            packet_type = self.packet_type,
            payload_len = payload_bytes.len(),
            total_len = buffer.len(),
            "Encoded packet"
        );

        Ok(buffer)
    }
}

/// Read a complete RCON packet from the stream
///
/// Uses `read_exact()` in two stages to handle TCP fragmentation:
/// 1. Read 4-byte length header
/// 2. Read exactly `length` bytes of packet body
pub(crate) async fn read_packet(stream: &mut TcpStream) -> Result<Packet> {
    // 1. Read length header (4 bytes)
    let mut length_buf = [0u8; 4];
    stream.read_exact(&mut length_buf).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            RconError::ConnectionLost(e)
        } else {
            RconError::Io(e)
        }
    })?;

    let length = i32::from_le_bytes(length_buf);

    // 2. Validate length (as i32, before casting to usize)
    if length < MIN_PACKET_SIZE {
        return Err(RconError::InvalidPacket(format!(
            "Packet body too small: {} bytes (minimum {})",
            length, MIN_PACKET_SIZE
        )));
    }

    if length > MAX_PACKET_SIZE {
        return Err(RconError::InvalidPacket(format!(
            "Packet body too large: {} bytes (maximum {})",
            length, MAX_PACKET_SIZE
        )));
    }

    let length = length as usize;

    debug!(body_length = length, "Reading packet");

    // 3. Read exactly `length` bytes (handles TCP fragmentation)
    let mut packet_buf = vec![0u8; length];
    stream.read_exact(&mut packet_buf).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            RconError::ConnectionLost(e)
        } else {
            RconError::Io(e)
        }
    })?;

    // 4. Parse packet buffer
    parse_packet_buffer(&packet_buf)
}

/// Parse a packet from raw bytes (after length header has been consumed)
fn parse_packet_buffer(buffer: &[u8]) -> Result<Packet> {
    if buffer.len() < MIN_PACKET_SIZE as usize {
        return Err(RconError::InvalidPacket(format!(
            "Buffer too small: {} bytes",
            buffer.len()
        )));
    }

    let id = i32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
    let packet_type = i32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);

    // Payload is everything between header (8 bytes) and null terminator (2 bytes)
    let payload_end = buffer.len() - 2;
    let payload_bytes = &buffer[8..payload_end];
    let payload = String::from_utf8_lossy(payload_bytes).to_string();

    debug!(
        id,
        packet_type,
        payload_len = payload.len(),
        "Parsed packet"
    );

    Ok(Packet {
        id,
        packet_type,
        payload,
    })
}

/// Write a packet to the stream
pub(crate) async fn write_packet(stream: &mut TcpStream, packet: &Packet) -> Result<()> {
    let encoded = packet.encode()?;
    stream.write_all(&encoded).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_auth_packet() {
        let packet = Packet::auth(1, "password");
        let encoded = packet.encode().unwrap();

        // Length (18 bytes: 4 ID + 4 type + 8 "password" + 2 null)
        assert_eq!(
            i32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]),
            18
        );

        // ID
        assert_eq!(
            i32::from_le_bytes([encoded[4], encoded[5], encoded[6], encoded[7]]),
            1
        );

        // Type (auth = 3)
        assert_eq!(
            i32::from_le_bytes([encoded[8], encoded[9], encoded[10], encoded[11]]),
            3
        );

        // Payload
        assert_eq!(&encoded[12..20], b"password");

        // Null terminator
        assert_eq!(&encoded[20..22], &[0, 0]);
    }

    #[test]
    fn test_encode_command_packet() {
        let packet = Packet::command(2, "/version");
        let encoded = packet.encode().unwrap();

        // Length (18 bytes: 4 ID + 4 type + 8 "/version" + 2 null)
        assert_eq!(
            i32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]),
            18
        );

        // ID
        assert_eq!(
            i32::from_le_bytes([encoded[4], encoded[5], encoded[6], encoded[7]]),
            2
        );

        // Type (command = 2)
        assert_eq!(
            i32::from_le_bytes([encoded[8], encoded[9], encoded[10], encoded[11]]),
            2
        );

        // Payload
        assert_eq!(&encoded[12..20], b"/version");
    }

    #[test]
    fn test_encode_empty_payload() {
        let packet = Packet::command(1, "");
        let encoded = packet.encode().unwrap();

        // Length (10 bytes: 4 ID + 4 type + 0 payload + 2 null)
        assert_eq!(
            i32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]),
            10
        );

        // Total: 4 length + 10 body = 14 bytes
        assert_eq!(encoded.len(), 14);
    }

    #[test]
    fn test_parse_valid_packet() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&5i32.to_le_bytes()); // ID = 5
        buffer.extend_from_slice(&0i32.to_le_bytes()); // Type = 0 (response)
        buffer.extend_from_slice(b"test response"); // Payload
        buffer.extend_from_slice(&[0, 0]); // Null terminator

        let packet = parse_packet_buffer(&buffer).unwrap();

        assert_eq!(packet.id, 5);
        assert_eq!(packet.packet_type, 0);
        assert_eq!(packet.payload, "test response");
    }

    #[test]
    fn test_parse_empty_payload() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&1i32.to_le_bytes()); // ID
        buffer.extend_from_slice(&0i32.to_le_bytes()); // Type
        buffer.extend_from_slice(&[0, 0]); // Null terminator only

        let packet = parse_packet_buffer(&buffer).unwrap();

        assert_eq!(packet.id, 1);
        assert_eq!(packet.payload, "");
    }

    #[test]
    fn test_reject_too_small_buffer() {
        let buffer = vec![0u8; 8];
        let result = parse_packet_buffer(&buffer);
        assert!(matches!(result, Err(RconError::InvalidPacket(_))));
    }

    #[test]
    fn test_parse_payload_with_embedded_nulls() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&1i32.to_le_bytes()); // ID
        buffer.extend_from_slice(&0i32.to_le_bytes()); // Type
        buffer.extend_from_slice(b"hello\x00world"); // Payload with embedded null
        buffer.extend_from_slice(&[0, 0]); // Null terminator

        let packet = parse_packet_buffer(&buffer).unwrap();
        assert_eq!(packet.payload, "hello\0world");
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let original = Packet::command(42, "test command");
        let encoded = original.encode().unwrap();

        // Skip the length field (first 4 bytes) for parsing
        let parsed = parse_packet_buffer(&encoded[4..]).unwrap();

        assert_eq!(parsed.id, original.id);
        assert_eq!(parsed.packet_type, original.packet_type);
        assert_eq!(parsed.payload, original.payload);
    }

    #[test]
    fn test_negative_id_roundtrip() {
        // Auth failure responses have ID = -1
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&(-1i32).to_le_bytes()); // ID = -1
        buffer.extend_from_slice(&2i32.to_le_bytes()); // Type
        buffer.extend_from_slice(&[0, 0]); // Null terminator

        let packet = parse_packet_buffer(&buffer).unwrap();
        assert_eq!(packet.id, -1);
    }
}

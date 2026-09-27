//! PTP/MTP container wire format and operation codes.
//!
//! MTP is built on top of the PTP (Picture Transfer Protocol, ISO 15740)
//! transport layer. Communication uses USB bulk endpoints with a simple
//! container structure:
//!
//! ```text
//! ┌──────────┬──────────┬──────────┬───────────────┬─────────┐
//! │ Length   │ Type     │ Code     │ TransactionID │ Payload │
//! │ u32 LE  │ u16 LE   │ u16 LE   │ u32 LE        │ ...     │
//! └──────────┴──────────┴──────────┴───────────────┴─────────┘
//! ```
//!
//! The header is always 12 bytes. `Length` includes the header.

use thiserror::Error;

// ── Container types ─────────────────────────────────────────────

pub const CONTAINER_COMMAND: u16 = 1;
pub const CONTAINER_DATA: u16 = 2;
pub const CONTAINER_RESPONSE: u16 = 3;
#[allow(dead_code)]
pub const CONTAINER_EVENT: u16 = 4;

pub const HEADER_LEN: usize = 12;

// ── PTP/MTP operation codes ─────────────────────────────────────

pub const OP_GET_DEVICE_INFO: u16 = 0x1001;
pub const OP_OPEN_SESSION: u16 = 0x1002;
pub const OP_CLOSE_SESSION: u16 = 0x1003;
pub const OP_GET_STORAGE_IDS: u16 = 0x1004;
pub const OP_GET_STORAGE_INFO: u16 = 0x1005;
#[allow(dead_code)]
pub const OP_GET_NUM_OBJECTS: u16 = 0x1006;
pub const OP_GET_OBJECT_HANDLES: u16 = 0x1007;
pub const OP_GET_OBJECT_INFO: u16 = 0x1008;
pub const OP_GET_OBJECT: u16 = 0x1009;
pub const OP_DELETE_OBJECT: u16 = 0x100B;
pub const OP_SEND_OBJECT_INFO: u16 = 0x100C;
pub const OP_SEND_OBJECT: u16 = 0x100D;

// ── MTP-specific format codes ───────────────────────────────────

/// Association (folder) format code.
pub const FORMAT_ASSOCIATION: u16 = 0x3001;
/// Undefined format — used for generic files.
#[allow(dead_code)]
pub const FORMAT_UNDEFINED: u16 = 0x3000;

// ── PTP response codes ──────────────────────────────────────────

pub const RC_OK: u16 = 0x2001;
#[allow(dead_code)]
pub const RC_SESSION_ALREADY_OPEN: u16 = 0x201E;

// ── Error type ──────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum MtpError {
    #[error("USB error: {0}")]
    Usb(String),

    #[error("MTP protocol error: response code 0x{0:04X}")]
    Protocol(u16),

    #[error("Invalid data received from device: {0}")]
    InvalidData(String),

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("No active session for device {0}")]
    NoSession(String),

    #[error("Transfer cancelled")]
    Cancelled,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Container building / parsing ────────────────────────────────

/// Build a PTP command container with up to 5 u32 parameters.
pub fn build_command(opcode: u16, transaction_id: u32, params: &[u32]) -> Vec<u8> {
    let payload_len = params.len() * 4;
    let total_len = HEADER_LEN + payload_len;
    let mut buf = Vec::with_capacity(total_len);

    buf.extend_from_slice(&(total_len as u32).to_le_bytes());
    buf.extend_from_slice(&CONTAINER_COMMAND.to_le_bytes());
    buf.extend_from_slice(&opcode.to_le_bytes());
    buf.extend_from_slice(&transaction_id.to_le_bytes());

    for p in params {
        buf.extend_from_slice(&p.to_le_bytes());
    }

    buf
}

/// Build a PTP data container with a payload.
pub fn build_data(opcode: u16, transaction_id: u32, payload: &[u8]) -> Vec<u8> {
    let total_len = HEADER_LEN + payload.len();
    let mut buf = Vec::with_capacity(total_len);

    buf.extend_from_slice(&(total_len as u32).to_le_bytes());
    buf.extend_from_slice(&CONTAINER_DATA.to_le_bytes());
    buf.extend_from_slice(&opcode.to_le_bytes());
    buf.extend_from_slice(&transaction_id.to_le_bytes());
    buf.extend_from_slice(payload);

    buf
}

/// Parsed response or data container.
#[derive(Debug)]
pub struct Container {
    pub container_type: u16,
    pub code: u16,
    pub transaction_id: u32,
    pub payload: Vec<u8>,
}

/// Parse a raw USB bulk-in buffer into a Container.
pub fn parse_container(raw: &[u8]) -> Result<Container, MtpError> {
    if raw.len() < HEADER_LEN {
        return Err(MtpError::InvalidData(format!(
            "container too short: {} bytes",
            raw.len()
        )));
    }

    let length = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    let container_type = u16::from_le_bytes([raw[4], raw[5]]);
    let code = u16::from_le_bytes([raw[6], raw[7]]);
    let transaction_id = u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]);

    let payload_end = length.min(raw.len());
    let payload = if payload_end > HEADER_LEN {
        raw[HEADER_LEN..payload_end].to_vec()
    } else {
        Vec::new()
    };

    Ok(Container {
        container_type,
        code,
        transaction_id,
        payload,
    })
}

// ── Payload parsing helpers ─────────────────────────────────────

/// Read a u32 little-endian value from a byte slice at the given offset.
pub fn read_u32(data: &[u8], offset: usize) -> Result<u32, MtpError> {
    if offset + 4 > data.len() {
        return Err(MtpError::InvalidData("u32 read out of bounds".into()));
    }
    Ok(u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

/// Read a u16 little-endian value from a byte slice at the given offset.
pub fn read_u16(data: &[u8], offset: usize) -> Result<u16, MtpError> {
    if offset + 2 > data.len() {
        return Err(MtpError::InvalidData("u16 read out of bounds".into()));
    }
    Ok(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

/// Read a u64 little-endian value from a byte slice at the given offset.
pub fn read_u64(data: &[u8], offset: usize) -> Result<u64, MtpError> {
    if offset + 8 > data.len() {
        return Err(MtpError::InvalidData("u64 read out of bounds".into()));
    }
    Ok(u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ]))
}

/// Read a PTP-style string: first byte is length (in UTF-16 code units),
/// followed by that many u16 values (little-endian), including a null
/// terminator.
pub fn read_ptp_string(data: &[u8], offset: usize) -> Result<(String, usize), MtpError> {
    if offset >= data.len() {
        return Ok((String::new(), 1));
    }

    let num_chars = data[offset] as usize;
    if num_chars == 0 {
        return Ok((String::new(), 1));
    }

    let byte_len = num_chars * 2;
    let start = offset + 1;
    let end = start + byte_len;

    if end > data.len() {
        return Err(MtpError::InvalidData("PTP string overflows buffer".into()));
    }

    let mut utf16 = Vec::with_capacity(num_chars);
    for i in (start..end).step_by(2) {
        utf16.push(u16::from_le_bytes([data[i], data[i + 1]]));
    }

    // Remove null terminator if present
    if let Some(&0) = utf16.last() {
        utf16.pop();
    }

    let s = String::from_utf16_lossy(&utf16);
    Ok((s, 1 + byte_len))
}

/// Read a PTP u32 array: first u32 is element count, followed by that
/// many u32 values.
pub fn read_u32_array(data: &[u8], offset: usize) -> Result<Vec<u32>, MtpError> {
    let count = read_u32(data, offset)? as usize;
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        result.push(read_u32(data, offset + 4 + i * 4)?);
    }
    Ok(result)
}

/// Encode a PTP-style string (length-prefixed UTF-16LE with null terminator).
pub fn encode_ptp_string(s: &str) -> Vec<u8> {
    let utf16: Vec<u16> = s.encode_utf16().chain(std::iter::once(0u16)).collect();
    let num_chars = utf16.len() as u8;
    let mut buf = Vec::with_capacity(1 + utf16.len() * 2);
    buf.push(num_chars);
    for ch in utf16 {
        buf.extend_from_slice(&ch.to_le_bytes());
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_and_parse_command() {
        let cmd = build_command(OP_OPEN_SESSION, 1, &[1]);
        assert_eq!(cmd.len(), 16); // 12 header + 4 param

        let container = parse_container(&cmd).expect("valid container");
        assert_eq!(container.container_type, CONTAINER_COMMAND);
        assert_eq!(container.code, OP_OPEN_SESSION);
        assert_eq!(container.transaction_id, 1);
        assert_eq!(container.payload, 1u32.to_le_bytes().to_vec());
    }

    #[test]
    fn test_build_and_parse_data() {
        let payload = b"Hello, Android!";
        let data = build_data(OP_SEND_OBJECT, 42, payload);
        assert_eq!(data.len(), 12 + payload.len());

        let container = parse_container(&data).expect("valid container");
        assert_eq!(container.container_type, CONTAINER_DATA);
        assert_eq!(container.code, OP_SEND_OBJECT);
        assert_eq!(container.transaction_id, 42);
        assert_eq!(container.payload, payload);
    }

    #[test]
    fn test_ptp_string_roundtrip() {
        let original = "DCIM_Camera_Photos";
        let encoded = encode_ptp_string(original);
        let (decoded, consumed) = read_ptp_string(&encoded, 0).expect("decode string");
        assert_eq!(decoded, original);
        assert_eq!(consumed, encoded.len());
    }

    #[test]
    fn test_empty_ptp_string() {
        let encoded = vec![0u8];
        let (decoded, consumed) = read_ptp_string(&encoded, 0).expect("empty string");
        assert_eq!(decoded, "");
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_read_u32_array() {
        let mut buf = Vec::new();
        // count = 3
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&100u32.to_le_bytes());
        buf.extend_from_slice(&200u32.to_le_bytes());
        buf.extend_from_slice(&300u32.to_le_bytes());

        let result = read_u32_array(&buf, 0).expect("read u32 array");
        assert_eq!(result, vec![100, 200, 300]);
    }

    #[test]
    fn test_container_too_short() {
        let truncated = vec![0u8; 8];
        assert!(parse_container(&truncated).is_err());
    }
}


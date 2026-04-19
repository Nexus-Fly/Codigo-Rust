use crate::types::RawTransaction;
use anyhow::Result;

/// Encode an arbitrary string into a null-terminated transaction payload,
/// matching the convention already used in `main.rs`.
pub fn encode_str(s: &str) -> RawTransaction {
    let mut buf = vec![0u8; s.len() + 1];
    buf[..s.len()].copy_from_slice(s.as_bytes());
    buf
}

/// Attempt to decode a raw transaction payload as a UTF-8 string,
/// stripping any trailing null byte.
pub fn decode_str(raw: &[u8]) -> Result<String> {
    let trimmed = raw.strip_suffix(b"\0").unwrap_or(raw);
    Ok(std::str::from_utf8(trimmed)?.to_owned())
}

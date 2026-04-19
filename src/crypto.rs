use sha2::{Digest, Sha256};

/// Compute proof-of-delivery hash: SHA-256(geo_lat || geo_lon || timestamp_bytes || order_id_bytes)
pub fn proof_hash(lat: f64, lon: f64, timestamp_ms: u64, order_id: u64) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(lat.to_le_bytes());
    hasher.update(lon.to_le_bytes());
    hasher.update(timestamp_ms.to_le_bytes());
    hasher.update(order_id.to_le_bytes());
    hasher.finalize().to_vec()
}

/// Lightweight HMAC-style signature using SHA-256(secret_key || message).
/// In production replace with Ed25519 via tashi_vertex::KeySecret.
pub fn sign_message(secret_key_bytes: &[u8], message: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(secret_key_bytes);
    hasher.update(message);
    hasher.finalize().to_vec()
}

/// Verify a signature produced by `sign_message`.
pub fn verify_signature(secret_key_bytes: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let expected = sign_message(secret_key_bytes, message);
    expected == signature
}

/// Generate a deterministic 32-byte "secret key" from a seed string (demo only).
pub fn derive_key(seed: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    hasher.finalize().to_vec()
}

/// Produce a client escrow signature: SHA-256(order_id || amount).
pub fn client_escrow_signature(order_id: u64, amount: u64) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(order_id.to_le_bytes());
    hasher.update(amount.to_le_bytes());
    hasher.finalize().to_vec()
}

/// Validator signature (BFT quorum member signs escrow_id || to_agent || amount).
pub fn validator_signature(
    validator_key: &[u8],
    escrow_id: u64,
    to_agent: &str,
    amount: u64,
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(validator_key);
    hasher.update(escrow_id.to_le_bytes());
    hasher.update(to_agent.as_bytes());
    hasher.update(amount.to_le_bytes());
    hasher.finalize().to_vec()
}

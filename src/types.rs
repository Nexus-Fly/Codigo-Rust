/// Node identifier derived from its public key (Base58 string).
pub type NodeId = String;

/// Raw transaction bytes as produced / consumed by Tashi Vertex.
pub type RawTransaction = Vec<u8>;

/// Logical round / consensus height counter.
pub type Round = u64;

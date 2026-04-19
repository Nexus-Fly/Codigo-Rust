use crate::types::Round;

/// Auction state for a given consensus round.
#[derive(Debug, Default)]
pub struct Auction {
    pub round: Round,
}

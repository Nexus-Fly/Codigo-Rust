use crate::types::Round;

/// Append-only ledger of confirmed consensus rounds.
#[derive(Debug, Default)]
pub struct Ledger {
    pub confirmed: Vec<Round>,
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a newly confirmed round.
    pub fn commit(&mut self, round: Round) {
        self.confirmed.push(round);
    }
}

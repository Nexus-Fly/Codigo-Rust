use anyhow::Result;

/// Simulation runner – drives a local multi-node scenario for testing.
/// Stub only; no simulation logic yet.
#[derive(Debug, Default)]
pub struct Runner;

impl Runner {
    pub fn new() -> Self {
        Self
    }

    /// Run the simulation. Currently a no-op placeholder.
    pub async fn run(&self) -> Result<()> {
        // TODO: drive simulated nodes through consensus rounds
        Ok(())
    }
}

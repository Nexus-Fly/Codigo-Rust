use anyhow::Result;

/// Safety gate: returns `Ok(())` when the proposed action is deemed safe.
/// Stub – always approves until real rules are defined.
pub fn check(_action: &str) -> Result<()> {
    Ok(())
}

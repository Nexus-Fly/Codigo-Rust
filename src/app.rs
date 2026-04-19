use crate::{config::Config, store::Store};

/// Top-level application handle. Wires configuration to runtime components.
pub struct App {
    pub config: Config,
    pub store: Store,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            store: Store::new(),
        }
    }
}

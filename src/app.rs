use crate::{config::AppConfig, store::Store};

/// Top-level application handle. Wires configuration to runtime components.
pub struct App {
    pub config: AppConfig,
    pub store: Store,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            store: Store::new(),
        }
    }
}

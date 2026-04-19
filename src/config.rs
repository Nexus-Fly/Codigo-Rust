/// Application-level configuration, parsed once at startup.
#[derive(Debug, Clone)]
pub struct Config {
    /// Local bind address (e.g. "127.0.0.1:8001")
    pub bind: String,
    /// Base58-encoded secret key for this node
    pub key: String,
    /// Remote peers in "<public_key>@<ip:port>" format
    pub peers: Vec<String>,
    /// Initial message/transaction sent on startup
    pub initial_message: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8001".into(),
            key: String::new(),
            peers: Vec::new(),
            initial_message: "PING".into(),
        }
    }
}

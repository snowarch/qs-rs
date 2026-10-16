//! Process-wide state shared by the Quickshell modules.

use std::sync::OnceLock;

static INITIAL_ENV: OnceLock<Vec<(String, String)>> = OnceLock::new();

/// Snapshot the environment before shell pragmas modify it. Child processes start from this.
pub fn capture_initial_env() {
    INITIAL_ENV.get_or_init(|| std::env::vars().collect());
}

pub fn initial_env() -> &'static [(String, String)] {
    INITIAL_ENV.get_or_init(|| std::env::vars().collect())
}

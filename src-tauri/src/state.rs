//! Application state shared across Tauri commands via `tauri::State`.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::mtp::session::MtpSession;

/// Thread-safe application state.
pub struct AppState {
    /// Active MTP sessions keyed by device ID.
    pub sessions: Mutex<HashMap<String, MtpSession>>,
    /// Cancellation flags for in-flight transfers, keyed by transfer ID.
    pub cancel_flags: Mutex<HashMap<String, bool>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            cancel_flags: Mutex::new(HashMap::new()),
        }
    }
}

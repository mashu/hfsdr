//! Engine mirror on the UI thread (from [`EngineHandle::try_poll`]).

use crate::engine::{ConnState, EngineStats};

#[derive(Debug)]
pub struct EngineUiState {
    pub conn_state: ConnState,
    pub stats: EngineStats,
    pub last_error: Option<String>,
}

impl Default for EngineUiState {
    fn default() -> Self {
        Self {
            conn_state: ConnState::Disconnected,
            stats: EngineStats::default(),
            last_error: None,
        }
    }
}

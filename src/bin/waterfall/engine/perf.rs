//! Pipeline profiling toggles (UI param + `HFSDR_PERF` env).

use std::sync::OnceLock;

use super::types::EngineParams;

static ENV_PERF: OnceLock<bool> = OnceLock::new();

pub fn env_perf_enabled() -> bool {
    *ENV_PERF.get_or_init(|| {
        std::env::var("HFSDR_PERF")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

pub fn perf_enabled(params: &EngineParams) -> bool {
    params.perf_trace || env_perf_enabled()
}

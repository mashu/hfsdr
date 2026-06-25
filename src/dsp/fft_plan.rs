//! Cached forward/inverse FFT plans — avoids a fresh [`FftPlanner`] on every resize.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use rustfft::{Fft, FftPlanner};

type PlanMap = HashMap<usize, Arc<dyn Fft<f32>>>;

fn forward_cache() -> &'static Mutex<PlanMap> {
    static CACHE: OnceLock<Mutex<PlanMap>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn inverse_cache() -> &'static Mutex<PlanMap> {
    static CACHE: OnceLock<Mutex<PlanMap>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Shared forward complex FFT plan for power-of-two `n`.
pub fn plan_forward(n: usize) -> Arc<dyn Fft<f32>> {
    let mut guard = forward_cache().lock().expect("fft plan cache");
    if let Some(plan) = guard.get(&n) {
        return Arc::clone(plan);
    }
    let mut planner = FftPlanner::<f32>::new();
    let plan = planner.plan_fft_forward(n);
    guard.insert(n, Arc::clone(&plan));
    plan
}

/// Shared inverse complex FFT plan for power-of-two `n`.
pub fn plan_inverse(n: usize) -> Arc<dyn Fft<f32>> {
    let mut guard = inverse_cache().lock().expect("fft inv plan cache");
    if let Some(plan) = guard.get(&n) {
        return Arc::clone(plan);
    }
    let mut planner = FftPlanner::<f32>::new();
    let plan = planner.plan_fft_inverse(n);
    guard.insert(n, Arc::clone(&plan));
    plan
}

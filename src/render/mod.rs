//! Shared GPU rendering for any egui-based frontend.
//!
//! Lives in the library rather than the desktop binary so the browser build and
//! the native build paint the same waterfall from the same shader — duplicating
//! it is exactly the drift that hid the colormap cost in the first place.

pub mod waterfall_gpu;

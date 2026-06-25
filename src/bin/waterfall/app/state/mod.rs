//! Grouped `WaterfallApp` state (connection form, plot, skimmer UI, …).

mod audio;
mod chrome;
mod connection;
mod display;
mod plot;
mod radio;
mod skimmer;

pub use audio::AudioUiState;
pub use chrome::ChromeState;
pub use connection::ConnectionState;
pub use display::DisplayState;
pub use plot::PlotState;
pub use radio::RadioState;
pub use skimmer::SkimmerUiState;

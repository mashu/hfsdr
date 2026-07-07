#[derive(Clone, Debug)]
pub struct AudioUiState {
    pub audio_devices: Vec<String>,
    pub selected_audio_device: usize,
    pub last_audio_device: usize,
    pub audio_enabled: bool,
    pub volume: f32,
    pub audio_scope: Vec<f32>,
    pub audio_waveform: Vec<f32>,
}

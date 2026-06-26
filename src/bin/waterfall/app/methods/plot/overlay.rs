use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {
    pub(crate) fn overlay_audio_rate(&self) -> f32 {
        hfsdr::audio_sample_rate(
            self.radio.sample_rate.max(self.engine_ui.stats.sample_rate),
            self.radio.cw.decimation,
        )
    }

    pub(crate) fn filter_overlay_cached(&mut self) -> &hfsdr::FilterOverlay {
        let audio_rate = self.overlay_audio_rate();
        let key = hfsdr::filter_overlay_cache_key(&self.radio.cw, audio_rate);
        if self.plot.filter_overlay.key != key {
            self.plot.filter_overlay.overlay =
                hfsdr::build_filter_overlay(&self.radio.cw, audio_rate);
            self.plot.filter_overlay.key = key;
        }
        &self.plot.filter_overlay.overlay
    }
}

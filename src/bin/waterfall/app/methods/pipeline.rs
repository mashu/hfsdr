use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn toggle_pipeline_stage(&mut self, stage: PipelineStage) {
        match stage {
            PipelineStage::NoiseBlanker => {
                self.radio.cw.noise_blanker.enabled = !self.radio.cw.noise_blanker.enabled;
            }
            PipelineStage::ManualNotches => self.toggle_notch_bypass(),
            PipelineStage::ListenNco => {
                self.radio.cw.diagnostic.listen_nco = !self.radio.cw.diagnostic.listen_nco;
            }
            PipelineStage::DecimatorFir => {
                self.radio.cw.diagnostic.decim_fir = !self.radio.cw.diagnostic.decim_fir;
            }
            PipelineStage::ChannelFir => {
                self.radio.cw.diagnostic.channel_fir = !self.radio.cw.diagnostic.channel_fir;
            }
            PipelineStage::Bfo => {
                self.radio.cw.diagnostic.bfo = !self.radio.cw.diagnostic.bfo;
            }
            PipelineStage::Agc => self.radio.cw.agc.enabled = !self.radio.cw.agc.enabled,
            PipelineStage::Apf => self.radio.cw.apf.enabled = !self.radio.cw.apf.enabled,
            PipelineStage::AutoNotch => self.radio.cw.auto_notch.enabled = !self.radio.cw.auto_notch.enabled,
            PipelineStage::NoiseReduction => {
                self.radio.cw.noise_reduction.enabled = !self.radio.cw.noise_reduction.enabled;
            }
            PipelineStage::Skimmer => self.skimmer_ui.skimmer_enabled = !self.skimmer_ui.skimmer_enabled,
            PipelineStage::AudioOutput => self.audio.audio_enabled = !self.audio.audio_enabled,
        }
        let on = match stage {
            PipelineStage::NoiseBlanker => self.radio.cw.noise_blanker.enabled,
            PipelineStage::ManualNotches => self.radio.cw.notches.iter().any(|n| n.enabled),
            PipelineStage::ListenNco => !self.radio.cw.diagnostic.listen_nco,
            PipelineStage::DecimatorFir => !self.radio.cw.diagnostic.decim_fir,
            PipelineStage::ChannelFir => !self.radio.cw.diagnostic.channel_fir,
            PipelineStage::Bfo => !self.radio.cw.diagnostic.bfo,
            PipelineStage::Agc => self.radio.cw.agc.enabled,
            PipelineStage::Apf => self.radio.cw.apf.enabled,
            PipelineStage::AutoNotch => self.radio.cw.auto_notch.enabled,
            PipelineStage::NoiseReduction => self.radio.cw.noise_reduction.enabled,
            PipelineStage::Skimmer => self.skimmer_ui.skimmer_enabled,
            PipelineStage::AudioOutput => self.audio.audio_enabled,
        };
        let tag = if stage.is_diagnostic() { "diag" } else { "pipeline" };
        log::info(&format!(
            "{tag} {} {}",
            stage.label(),
            if on { "on" } else { "bypassed" }
        ));
        if !stage.is_diagnostic() {
            self.settings_dirty_at = Some(Instant::now());
        }
    }



    pub(crate) fn toggle_notch_bypass(&mut self) {
        let any = self.radio.cw.notches.iter().any(|n| n.enabled);
        if any {
            let mut stash = [false; MAX_NOTCHES];
            for (slot, n) in self.radio.cw.notches.iter_mut().enumerate() {
                stash[slot] = n.enabled;
                n.enabled = false;
            }
            self.chrome.notch_bypass_stash = Some(stash);
            return;
        }
        if let Some(stash) = self.chrome.notch_bypass_stash.take() {
            for (n, was) in self.radio.cw.notches.iter_mut().zip(stash.iter()) {
                n.enabled = *was;
            }
        }
    }



    pub(crate) fn arm_manual_notch(&mut self, slot: usize, offset_hz: Option<ChannelOffsetHz>) {
        let listen = ChannelOffsetHz::new(self.listen_offset_hz() as f32);
        let other: Vec<ChannelOffsetHz> = self
            .radio.cw
            .notches
            .iter()
            .enumerate()
            .filter(|(i, n)| *i != slot && n.enabled)
            .map(|(_, n)| n.offset_hz)
            .collect();
        let offset = offset_hz.unwrap_or_else(|| suggest_notch_offset_hz(listen, &other));
        let Some(notch) = self.radio.cw.notches.get_mut(slot) else {
            return;
        };
        notch.enabled = true;
        notch.offset_hz = offset;
        if notch.width_hz < NOTCH_WIDTH_MIN_HZ {
            notch.width_hz = 50.0;
        }
        self.chrome.notch_bypass_stash = None;
    }



    pub(crate) fn enabled_notches(&self, overlay: &hfsdr::FilterOverlay) -> Vec<crate::interaction::NotchMarker> {
        self.radio.cw
            .notches
            .iter()
            .enumerate()
            .filter(|(_, n)| n.enabled)
            .map(|(slot, n)| crate::interaction::NotchMarker {
                slot,
                offset_hz: n.offset_hz,
                display_half_hz: overlay.notch_half_hz[slot],
            })
            .collect()
    }



    pub(crate) fn toggle_manual_notch(&mut self, slot: usize) {
        if slot >= MAX_NOTCHES {
            return;
        }
        if self.radio.cw.notches[slot].enabled {
            self.radio.cw.notches[slot].enabled = false;
        } else {
            self.arm_manual_notch(slot, None);
        }
    }



    pub(crate) fn pipeline_ingress_decim(&self) -> usize {
        let device_rate = if self.engine_ui.stats.sample_rate > 0.0 {
            self.engine_ui.stats.sample_rate.round() as u32
        } else {
            self.connection.form.sample_rate
        };
        match self.connection.form.kind {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => self.connection.form.airspy.ingress_decimation(device_rate).0,
            SourceKind::Kiwi => self.connection.form.kiwi.ingress_decimation(device_rate).0,
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => self.connection.form.rtlsdr.ingress_decimation(device_rate).0,
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => self.connection.form.qmx.ingress_decimation(device_rate).0,
            #[cfg(feature = "soapy")]
            SourceKind::Soapy => self.connection.form.soapy.ingress_decimation(device_rate).0,
        }
    }



    pub(crate) fn process_iq_cmds(&mut self, cmds: Vec<IqPanelCmd>) {
        for cmd in cmds {
            match cmd {
                IqPanelCmd::StartRecord(path) => {
                    self.engine.send(EngineCommand::StartIqRecord(path));
                }
                IqPanelCmd::StopRecord => {
                    self.engine.send(EngineCommand::StopIqRecord);
                }
                IqPanelCmd::Play(path) => {
                    if let Ok(meta) = hfsdr::read_meta(&path) {
                        self.radio.center_khz = meta.center_hz / 1000.0;
                        self.plot.plot_view.pan_offset_hz = 0.0;
                        self.clear_rit();
                    }
                    self.engine.send(EngineCommand::PlayIqFile(path));
                }
                IqPanelCmd::StopPlayback => {
                    self.engine.send(EngineCommand::StopIqPlayback);
                }
            }
        }
    }


}

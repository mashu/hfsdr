// `pipeline` — `WaterfallApp` methods.

    fn toggle_pipeline_stage(&mut self, stage: PipelineStage) {
        match stage {
            PipelineStage::NoiseBlanker => {
                self.cw.noise_blanker.enabled = !self.cw.noise_blanker.enabled;
            }
            PipelineStage::ManualNotches => self.toggle_notch_bypass(),
            PipelineStage::ListenNco => {
                self.cw.diagnostic.listen_nco = !self.cw.diagnostic.listen_nco;
            }
            PipelineStage::DecimatorFir => {
                self.cw.diagnostic.decim_fir = !self.cw.diagnostic.decim_fir;
            }
            PipelineStage::ChannelFir => {
                self.cw.diagnostic.channel_fir = !self.cw.diagnostic.channel_fir;
            }
            PipelineStage::Bfo => {
                self.cw.diagnostic.bfo = !self.cw.diagnostic.bfo;
            }
            PipelineStage::Agc => self.cw.agc.enabled = !self.cw.agc.enabled,
            PipelineStage::Apf => self.cw.apf.enabled = !self.cw.apf.enabled,
            PipelineStage::AutoNotch => self.cw.auto_notch.enabled = !self.cw.auto_notch.enabled,
            PipelineStage::NoiseReduction => {
                self.cw.noise_reduction.enabled = !self.cw.noise_reduction.enabled;
            }
            PipelineStage::Skimmer => self.skimmer_enabled = !self.skimmer_enabled,
            PipelineStage::AudioOutput => self.audio_enabled = !self.audio_enabled,
        }
        let on = match stage {
            PipelineStage::NoiseBlanker => self.cw.noise_blanker.enabled,
            PipelineStage::ManualNotches => self.cw.notches.iter().any(|n| n.enabled),
            PipelineStage::ListenNco => !self.cw.diagnostic.listen_nco,
            PipelineStage::DecimatorFir => !self.cw.diagnostic.decim_fir,
            PipelineStage::ChannelFir => !self.cw.diagnostic.channel_fir,
            PipelineStage::Bfo => !self.cw.diagnostic.bfo,
            PipelineStage::Agc => self.cw.agc.enabled,
            PipelineStage::Apf => self.cw.apf.enabled,
            PipelineStage::AutoNotch => self.cw.auto_notch.enabled,
            PipelineStage::NoiseReduction => self.cw.noise_reduction.enabled,
            PipelineStage::Skimmer => self.skimmer_enabled,
            PipelineStage::AudioOutput => self.audio_enabled,
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



    fn toggle_notch_bypass(&mut self) {
        let any = self.cw.notches.iter().any(|n| n.enabled);
        if any {
            let mut stash = [false; MAX_NOTCHES];
            for (slot, n) in self.cw.notches.iter_mut().enumerate() {
                stash[slot] = n.enabled;
                n.enabled = false;
            }
            self.notch_bypass_stash = Some(stash);
            return;
        }
        if let Some(stash) = self.notch_bypass_stash.take() {
            for (n, was) in self.cw.notches.iter_mut().zip(stash.iter()) {
                n.enabled = *was;
            }
        }
    }



    fn arm_manual_notch(&mut self, slot: usize, offset_hz: Option<ChannelOffsetHz>) {
        let listen = ChannelOffsetHz::new(self.listen_offset_hz() as f32);
        let other: Vec<ChannelOffsetHz> = self
            .cw
            .notches
            .iter()
            .enumerate()
            .filter(|(i, n)| *i != slot && n.enabled)
            .map(|(_, n)| n.offset_hz)
            .collect();
        let offset = offset_hz.unwrap_or_else(|| suggest_notch_offset_hz(listen, &other));
        let Some(notch) = self.cw.notches.get_mut(slot) else {
            return;
        };
        notch.enabled = true;
        notch.offset_hz = offset;
        if notch.width_hz < NOTCH_WIDTH_MIN_HZ {
            notch.width_hz = 50.0;
        }
        self.notch_bypass_stash = None;
    }



    fn enabled_notches(&self) -> Vec<crate::interaction::NotchMarker> {
        self.cw
            .notches
            .iter()
            .enumerate()
            .filter(|(_, n)| n.enabled)
            .map(|(slot, n)| crate::interaction::NotchMarker {
                slot,
                offset_hz: n.offset_hz,
                width_hz: n.width_hz,
            })
            .collect()
    }



    fn toggle_manual_notch(&mut self, slot: usize) {
        if slot >= MAX_NOTCHES {
            return;
        }
        if self.cw.notches[slot].enabled {
            self.cw.notches[slot].enabled = false;
        } else {
            self.arm_manual_notch(slot, None);
        }
    }



    fn pipeline_ingress_decim(&self) -> usize {
        let device_rate = if self.stats.sample_rate > 0.0 {
            self.stats.sample_rate.round() as u32
        } else {
            self.form_sample_rate
        };
        match self.form_kind {
            #[cfg(feature = "airspy")]
            SourceKind::Airspy => self.form_airspy.ingress_decimation(device_rate).0,
            SourceKind::Kiwi => self.form_kiwi.ingress_decimation(device_rate).0,
            #[cfg(feature = "rtlsdr")]
            SourceKind::RtlSdr => self.form_rtlsdr.ingress_decimation(device_rate).0,
            #[cfg(feature = "qmx")]
            SourceKind::Qmx => self.form_qmx.ingress_decimation(device_rate).0,
        }
    }



    fn process_iq_cmds(&mut self, cmds: Vec<IqPanelCmd>) {
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
                        self.center_khz = meta.center_hz / 1000.0;
                        self.plot_view.pan_offset_hz = 0.0;
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


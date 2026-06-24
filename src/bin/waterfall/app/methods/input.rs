// `input` — `WaterfallApp` methods.

    /// ←/→ pan the spectrogram view when zoomed; otherwise nudge RX center.
    /// Tap = `pan_step_hz`, sustained hold accelerates (2× then fast), Shift = fine, Ctrl = fast.
    fn handle_arrow_pan(&mut self, ctx: &egui::Context) {
        use egui::Key;

        let (left_down, right_down, left_press, right_press, shift, ctrl) = ctx.input(|i| {
            (
                i.key_down(Key::ArrowLeft),
                i.key_down(Key::ArrowRight),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.modifiers.shift,
                i.modifiers.ctrl || i.modifiers.command,
            )
        });

        if !left_down && !right_down {
            self.arrow_hold = None;
            return;
        }
        if !left_press && !right_press {
            return;
        }

        let direction = if left_press || (left_down && !right_down) {
            -1.0
        } else {
            1.0
        };

        let key = if direction < 0.0 {
            Key::ArrowLeft
        } else {
            Key::ArrowRight
        };
        let now = Instant::now();
        let hold = match self.arrow_hold {
            Some((held, started)) if held == key => now.saturating_duration_since(started),
            _ => {
                self.arrow_hold = Some((key, now));
                Duration::ZERO
            }
        };

        let base = self.pan_step_hz.max(10.0);
        let fast = self.pan_step_fast_hz.max(base);
        let step_hz = if ctrl {
            fast
        } else if shift {
            (base / 5.0).clamp(10.0, base)
        } else if hold >= Duration::from_millis(1200) {
            fast
        } else if hold >= Duration::from_millis(500) {
            (base * 2.0).clamp(base, fast)
        } else {
            base
        };

        let delta_hz = direction * step_hz as f64;
        let full_span = self.plot_full_span_hz();
        let max_zoom = self.plot_max_zoom_out();
        let can_pan = self.plot_view.can_pan(full_span, max_zoom);

        if can_pan || self.stats.iq_playback {
            self.plot_view.pan_offset_hz += delta_hz;
            self.plot_view.clamp_pan(full_span, max_zoom);
        } else {
            self.center_khz += delta_hz / 1000.0;
            self.clamp_center_to_ham_bands();
            self.apply_radio_settings();
        }
    }



    fn on_af_scope_panel_changed(&mut self) {
        if self.show_af_scope {
            self.show_right = true;
        }
    }



    fn toggle_af_scope(&mut self) {
        self.show_af_scope = !self.show_af_scope;
        self.on_af_scope_panel_changed();
    }



    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        self.handle_arrow_pan(ctx);
        let (
            zero,
            lock,
            notch,
            blank,
            nr,
            agc,
            apf,
            narrow,
            widen,
            rit_dn,
            rit_up,
            rit_clr,
            full,
            mute,
            vol_dn,
            vol_up,
            console,
            f11,
            overview,
            help,
            af_scope,
            notch1,
            notch2,
            notch3,
            notch4,
        ) = ctx.input(|i| {
            use egui::Key;
            (
                i.key_pressed(Key::Z),
                i.key_pressed(Key::L),
                i.key_pressed(Key::N),
                i.key_pressed(Key::B),
                i.key_pressed(Key::R),
                i.key_pressed(Key::A),
                i.key_pressed(Key::P),
                i.key_pressed(Key::OpenBracket),
                i.key_pressed(Key::CloseBracket),
                i.key_pressed(Key::Comma),
                i.key_pressed(Key::Period),
                i.key_pressed(Key::Backslash),
                i.key_pressed(Key::F),
                i.key_pressed(Key::Space),
                i.key_pressed(Key::Minus),
                i.key_pressed(Key::Equals),
                i.key_pressed(Key::Backtick),
                i.key_pressed(Key::F11),
                i.key_pressed(Key::M),
                i.key_pressed(Key::Questionmark),
                i.key_pressed(Key::G),
                i.key_pressed(Key::Num1),
                i.key_pressed(Key::Num2),
                i.key_pressed(Key::Num3),
                i.key_pressed(Key::Num4),
            )
        });

        if zero {
            self.zero_beat();
        }
        if lock {
            self.pitch_lock = !self.pitch_lock;
        }
        if notch {
            self.cw.auto_notch.enabled = !self.cw.auto_notch.enabled;
        }
        if blank {
            self.cw.noise_blanker.enabled = !self.cw.noise_blanker.enabled;
        }
        if nr {
            self.cw.noise_reduction.enabled = !self.cw.noise_reduction.enabled;
        }
        if agc {
            self.cw.agc.enabled = !self.cw.agc.enabled;
        }
        if apf {
            self.cw.apf.enabled = !self.cw.apf.enabled;
        }
        if narrow {
            self.cw.passband_hz =
                (self.cw.passband_hz - 25.0).clamp(CW_PASSBAND_MIN_HZ, self.passband_max_hz());
        }
        if widen {
            self.cw.passband_hz =
                (self.cw.passband_hz + 25.0).clamp(CW_PASSBAND_MIN_HZ, self.passband_max_hz());
        }
        if rit_dn {
            self.rit_hz = (self.rit_hz - 10.0).clamp(-800.0, 800.0);
        }
        if rit_up {
            self.rit_hz = (self.rit_hz + 10.0).clamp(-800.0, 800.0);
        }
        if rit_clr {
            self.clear_rit();
        }
        if full {
            self.plot_view.zoom_to_full_span();
        }
        if mute {
            self.audio_enabled = !self.audio_enabled;
        }
        if vol_dn {
            self.volume = (self.volume - 0.1).max(0.0);
        }
        if vol_up {
            self.volume = (self.volume + 0.1).min(4.0);
        }
        if console {
            self.show_console = !self.show_console;
        }
        if f11 {
            let on = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!on));
        }
        if overview {
            self.show_band_overview = !self.show_band_overview;
        }
        if help {
            self.show_shortcuts = !self.show_shortcuts;
        }
        if af_scope {
            self.toggle_af_scope();
        }
        if notch1 {
            self.toggle_manual_notch(0);
        }
        if notch2 {
            self.toggle_manual_notch(1);
        }
        if notch3 {
            self.toggle_manual_notch(2);
        }
        if notch4 {
            self.toggle_manual_notch(3);
        }
    }


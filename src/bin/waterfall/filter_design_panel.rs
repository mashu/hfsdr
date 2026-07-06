//! Compact channel filter design controls with live response preview.

use eframe::egui::{Color32, RichText, Ui};

use hfsdr::{
    channel_group_delay_ms, channel_magnitude_db_at, channel_half_width_hz, plan_num_taps,
    ChannelFilterKind, CwChannelSettings, IirFilterKind, MIN_KAISER_BETA, MAX_KAISER_BETA,
    OVERLAY_ATTEN_DB, WindowKind,
};

use crate::controls::scroll_slider_f32;
use crate::filter_curve_plot::paint_inline_response;
use crate::popup::segment_choice_sized;
use crate::theme::{attach_rich_tooltip, ACCENT, MUTED, OK};

pub struct FilterDesignPanel<'a> {
    pub settings: &'a mut CwChannelSettings,
    pub audio_rate: f32,
}

pub fn show_filter_design_panel(ui: &mut Ui, panel: FilterDesignPanel<'_>) {
    let audio_rate = panel.audio_rate.max(1.0);
    let settings = panel.settings;
    let passband_hz = settings.channel_bandwidth_hz();

    paint_inline_response(ui, settings, audio_rate, 88.0);

    let arch_sel = if settings.channel_filter == ChannelFilterKind::LinearFir {
        0
    } else {
        1
    };
    if let Some(i) = filter_segment_row(
        ui,
        "Architecture",
        "Channel filter architecture",
        architecture_tip(),
        "ch_filter_arch",
        arch_sel,
        &["FIR", "IIR 2-pole"],
    ) {
        settings.channel_filter = if i == 0 {
            ChannelFilterKind::LinearFir
        } else {
            ChannelFilterKind::Iir2Pole
        };
    }

    if settings.channel_filter == ChannelFilterKind::LinearFir {
        let shape_sel = match settings.window {
            WindowKind::Gaussian => 0,
            WindowKind::RaisedCosine => 1,
            WindowKind::Blackman => 2,
            WindowKind::Kaiser => 3,
        };
        if let Some(i) = filter_segment_row(
            ui,
            "Window",
            "FIR window (filter shape)",
            window_tip(),
            "ch_filter_win",
            shape_sel,
            &["Gauss", "RaisedCos", "Blackman", "Kaiser"],
        ) {
            settings.window = match i {
                1 => WindowKind::RaisedCosine,
                2 => WindowKind::Blackman,
                3 => WindowKind::Kaiser,
                _ => WindowKind::Gaussian,
            };
        }
        if settings.window == WindowKind::Kaiser {
            let beta_resp = scroll_slider_f32(
                ui,
                &mut settings.kaiser_beta,
                MIN_KAISER_BETA..=MAX_KAISER_BETA,
                "Kaiser β",
            );
            attach_rich_tooltip(
                &beta_resp,
                Some("Kaiser β"),
                &[
                    ("Stopband steepness", ACCENT),
                    (
                        "Higher β sharpens FIR skirts (better adjacent rejection); lower β \
                         is softer with a shorter impulse. β≈4 soft · β≈6 balanced · β≈10+ steep.",
                        MUTED,
                    ),
                ],
            );
        }
        let flatten_resp =
            ui.checkbox(&mut settings.passband_flatten, "Flatten passband (inv-sinc)");
        attach_rich_tooltip(
            &flatten_resp,
            Some("Flatten passband"),
            &[
                ("Inv-sinc lift", ACCENT),
                (
                    "Lifts upstream boxcar/CIC droop at band edges. Off by default — enable if \
                     the tone sounds dull when narrowed.",
                    MUTED,
                ),
            ],
        );
    } else {
        let iir_sel = match settings.iir_filter {
            IirFilterKind::Butterworth => 0,
            IirFilterKind::Chebyshev => 1,
        };
        if let Some(i) = filter_segment_row(
            ui,
            "IIR type",
            "IIR filter shape",
            iir_tip(),
            "ch_filter_iir",
            iir_sel,
            &["Butterworth", "Chebyshev"],
        ) {
            settings.iir_filter = if i == 1 {
                IirFilterKind::Chebyshev
            } else {
                IirFilterKind::Butterworth
            };
        }
    }

    paint_summary_line(ui, settings, audio_rate, passband_hz);
}

fn filter_segment_row(
    ui: &mut Ui,
    label: &str,
    title: &str,
    tip: &[(&str, Color32)],
    id: &str,
    selected: usize,
    options: &[&str],
) -> Option<usize> {
    let picked = ui.vertical(|ui| {
        label_with_tip(ui, label, title, tip);
        segment_choice_sized(ui, id, selected, options, 36.0)
    });
    picked.inner
}

fn label_with_tip(ui: &mut Ui, label: &str, title: &str, lines: &[(&str, Color32)]) {
    ui.spacing_mut().item_spacing.x = 4.0;
    let label_resp = ui.label(RichText::new(label).small().color(MUTED));
    let hint_resp = ui.label(RichText::new("(?)").small().color(MUTED));
    attach_rich_tooltip(&label_resp, Some(title), lines);
    attach_rich_tooltip(&hint_resp, Some(title), lines);
}

fn architecture_tip() -> &'static [(&'static str, Color32)] {
    &[
        ("IQ bandpass, pre-demod", ACCENT),
        (
            "Both options filter complex baseband before the BFO. Only energy inside the \
             passband is demodulated into audio.",
            MUTED,
        ),
        ("FIR", OK),
        (
            "Windowed sinc — linear phase, steep skirts, predictable group delay. \
             Best for fast CW; skirt shape set by Window below.",
            MUTED,
        ),
        ("IIR 2-pole", OK),
        (
            "Biquad lowpass — minimal delay and CPU, non-linear phase, may ring on fast keying. \
             Pick Butterworth or Chebyshev prototype.",
            MUTED,
        ),
    ]
}

fn window_tip() -> &'static [(&'static str, Color32)] {
    &[
        ("Shapes the channel FIR", ACCENT),
        (
            "Windowed-sinc bandpass on IQ. The window truncates the ideal sinc and sets how \
             sharply energy outside your passband is rejected.",
            MUTED,
        ),
        ("Why it affects audio", OK),
        (
            "Neighbors that leak through the skirts are mixed into your sidetone. BW sets width; \
             window sets skirt steepness vs keying ringing.",
            MUTED,
        ),
        ("Gaussian", OK),
        ("Softest — minimal ringing, gentle adjacent rejection.", MUTED),
        ("RaisedCos", OK),
        ("Everyday default — balanced tone and selectivity.", MUTED),
        ("Blackman", OK),
        ("Steepest clean skirts — best nearby-carrier rejection.", MUTED),
        ("Kaiser", OK),
        ("Tunable β — adjust steepness vs ringing.", MUTED),
    ]
}

fn iir_tip() -> &'static [(&'static str, Color32)] {
    &[
        ("2-pole prototype", ACCENT),
        (
            "Sets biquad Q for the IQ channel lowpass. FIR windows do not apply — shape comes \
             from the analog prototype.",
            MUTED,
        ),
        ("Butterworth", OK),
        ("Maximally flat passband — gentle, minimal peaking.", MUTED),
        ("Chebyshev", ACCENT),
        (
            "Steeper stopband with ~2 dB passband ripple — better adjacent rejection; may ring \
             more on keying.",
            MUTED,
        ),
    ]
}

fn paint_summary_line(ui: &mut Ui, settings: &CwChannelSettings, audio_rate: f32, passband_hz: f32) {
    let threshold = 10f32.powf(OVERLAY_ATTEN_DB / 20.0);
    let half_hz = channel_half_width_hz(settings, audio_rate, threshold);
    let adj_db = channel_magnitude_db_at(settings, audio_rate, passband_hz);

    let summary = if settings.channel_filter == ChannelFilterKind::LinearFir {
        let delay = channel_group_delay_ms(audio_rate, passband_hz);
        let taps = plan_num_taps(audio_rate, passband_hz);
        format!(
            "±{half_hz:.0} Hz (−3 dB) · {adj_db:.0} dB @ BW · ~{delay:.1} ms · {taps} taps"
        )
    } else {
        let kind = match settings.iir_filter {
            IirFilterKind::Chebyshev => "Chebyshev",
            IirFilterKind::Butterworth => "Butterworth",
        };
        format!("±{half_hz:.0} Hz (−3 dB) · {adj_db:.0} dB @ BW · {kind} IIR")
    };

    ui.label(RichText::new(summary).small().color(MUTED));
}

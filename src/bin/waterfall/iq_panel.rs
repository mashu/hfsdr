//! IQ capture and offline playback controls.

use std::path::PathBuf;

use eframe::egui::{self, Ui};

use crate::engine::EngineStats;
use crate::iq_dialog;
use crate::theme::{section_heading, section_hint, MUTED};

/// Persisted paths for IQ record / playback.
#[derive(Clone, Debug)]
pub struct IqPanel {
    pub capture_dir: PathBuf,
    pub playback_path: String,
    pub last_capture_path: String,
}

/// Commands produced by the panel each frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IqPanelCmd {
    StartRecord(PathBuf),
    StopRecord,
    Play(PathBuf),
    StopPlayback,
}

pub struct IqPanelView<'a> {
    pub stats: &'a EngineStats,
    pub streaming: bool,
}

impl IqPanel {
    pub fn new(capture_dir: PathBuf) -> Self {
        Self {
            capture_dir,
            playback_path: String::new(),
            last_capture_path: String::new(),
        }
    }

    /// Start a new timestamped capture in [`Self::capture_dir`].
    pub fn start_recording(&mut self) -> IqPanelCmd {
        let _ = std::fs::create_dir_all(&self.capture_dir);
        let path = hfsdr::timestamped_capture_path_in(&self.capture_dir);
        self.last_capture_path = path.display().to_string();
        if self.playback_path.is_empty() {
            self.playback_path = self.last_capture_path.clone();
        }
        IqPanelCmd::StartRecord(path)
    }

    /// Status-bar toggle: stop when recording, else start a fresh file when streaming.
    pub fn toggle_recording(&mut self, recording: bool, streaming: bool) -> Option<IqPanelCmd> {
        if recording {
            Some(IqPanelCmd::StopRecord)
        } else if streaming {
            Some(self.start_recording())
        } else {
            None
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        view: IqPanelView<'_>,
    ) -> (Vec<IqPanelCmd>, bool) {
        let mut cmds = Vec::new();
        let mut dirty = false;

        section_heading(ui, "IQ capture");
        section_hint(
            ui,
            "Records gzip-compressed .hiq.gz for offline replay — writer runs off the engine thread.",
        );

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Save to").small().color(MUTED));
            if ui
                .button("Browse…")
                .on_hover_text("Choose folder for new captures")
                .clicked()
            {
                if let Some(dir) = iq_dialog::pick_capture_dir(&self.capture_dir) {
                    self.capture_dir = dir;
                    dirty = true;
                }
            }
        });
        ui.label(
            egui::RichText::new(self.capture_dir.display().to_string())
                .small()
                .color(MUTED),
        );

        ui.horizontal(|ui| {
            if view.stats.iq_recording {
                if ui.button("Stop recording").clicked() {
                    cmds.push(IqPanelCmd::StopRecord);
                }
                if let Some(p) = &view.stats.iq_capture_path {
                    ui.label(
                        egui::RichText::new(format!(
                            "{:.1}s → {}",
                            view.stats.iq_capture_samples as f32
                                / view.stats.sample_rate.max(1.0),
                            p
                        ))
                        .small()
                        .color(MUTED),
                    );
                }
            } else if ui
                .add_enabled(view.streaming, egui::Button::new("Record IQ"))
                .on_hover_text("Start recording into the folder above")
                .clicked()
            {
                cmds.push(self.start_recording());
            }
        });

        if !self.last_capture_path.is_empty() && !view.stats.iq_recording {
            ui.label(
                egui::RichText::new(format!("Last: {}", self.last_capture_path))
                    .small()
                    .color(MUTED),
            );
        }

        ui.add_space(4.0);
        section_heading(ui, "IQ playback");
        section_hint(ui, "Replay a saved capture without a network link — buffer stays full (disk-fed).");

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("File").small().color(MUTED));
            if ui
                .button("Browse…")
                .on_hover_text("Open a .hiq.gz capture file")
                .clicked()
            {
                let start = if self.playback_path.is_empty() {
                    self.capture_dir.clone()
                } else {
                    PathBuf::from(&self.playback_path)
                };
                if let Some(path) = iq_dialog::pick_playback_file(&start) {
                    self.playback_path = path.display().to_string();
                    dirty = true;
                }
            }
        });
        let playback_label = if self.playback_path.is_empty() {
            "No file selected".to_string()
        } else {
            self.playback_path.clone()
        };
        ui.label(egui::RichText::new(playback_label).small().color(MUTED));

        ui.horizontal(|ui| {
            let can_play = !self.playback_path.trim().is_empty();
            if ui
                .add_enabled(can_play && !view.stats.iq_playback, egui::Button::new("Play"))
                .clicked()
            {
                cmds.push(IqPanelCmd::Play(PathBuf::from(
                    self.playback_path.trim(),
                )));
            }
            if view.stats.iq_playback && ui.button("Stop playback").clicked() {
                cmds.push(IqPanelCmd::StopPlayback);
            }
        });

        (cmds, dirty)
    }
}

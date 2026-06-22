//! IQ capture and offline playback controls.

use std::path::PathBuf;

use eframe::egui::Ui;

use crate::engine::EngineStats;
use crate::iq_dialog;
use crate::popup::{path_row, popup_section, primary_button, secondary_button};
use crate::theme::MUTED;

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

    pub fn start_recording(&mut self) -> IqPanelCmd {
        let _ = std::fs::create_dir_all(&self.capture_dir);
        let path = hfsdr::timestamped_capture_path_in(&self.capture_dir);
        self.last_capture_path = path.display().to_string();
        if self.playback_path.is_empty() {
            self.playback_path = self.last_capture_path.clone();
        }
        IqPanelCmd::StartRecord(path)
    }

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

        popup_section(ui, "Record", None, |ui| {
            if path_row(
                ui,
                "Folder",
                &self.capture_dir.display().to_string(),
                "…",
            ) {
                if let Some(dir) = iq_dialog::pick_capture_dir(&self.capture_dir) {
                    self.capture_dir = dir;
                    dirty = true;
                }
            }
            ui.horizontal(|ui| {
                if view.stats.iq_recording {
                    let label = view
                        .stats
                        .iq_capture_path
                        .as_deref()
                        .map(|p| {
                            let secs = view.stats.iq_capture_samples as f32
                                / view.stats.sample_rate.max(1.0);
                            format!("{secs:.0}s → {p}")
                        })
                        .unwrap_or_else(|| "Recording…".to_string());
                    ui.label(egui::RichText::new(label).small().color(MUTED));
                    if secondary_button(ui, "Stop").clicked() {
                        cmds.push(IqPanelCmd::StopRecord);
                    }
                } else if primary_button(ui, "Record", view.streaming).clicked() {
                    cmds.push(self.start_recording());
                }
            });
        });

        popup_section(ui, "Playback", None, |ui| {
            let playback_label = if self.playback_path.is_empty() {
                "(none)".to_string()
            } else {
                self.playback_path.clone()
            };
            if path_row(ui, "File", &playback_label, "…") {
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
            ui.horizontal(|ui| {
                let can_play = !self.playback_path.trim().is_empty();
                if primary_button(ui, "Play", can_play && !view.stats.iq_playback).clicked() {
                    cmds.push(IqPanelCmd::Play(PathBuf::from(
                        self.playback_path.trim(),
                    )));
                }
                if view.stats.iq_playback && secondary_button(ui, "Stop").clicked() {
                    cmds.push(IqPanelCmd::StopPlayback);
                }
            });
        });

        (cmds, dirty)
    }
}

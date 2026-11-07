//! Lightweight on-screen performance HUD (toggle with F12).

use std::{collections::VecDeque, time::Instant};

use eframe::egui::{self, Align2, Color32, CornerRadius, Margin, RichText, Stroke, vec2};

use crate::theme;

const SAMPLE_COUNT: usize = 240;

pub struct PerfHud {
    enabled: bool,
    last_frame: Option<Instant>,
    samples: VecDeque<f32>,
}

pub struct PerfInfo<'a> {
    pub cache_ready: bool,
    pub cache_path: Option<&'a str>,
    pub cache_error: Option<&'a str>,
    pub contacts_loaded: usize,
    pub contacts_total: usize,
    pub contacts_all: usize,
    pub last_query_ms: Option<f32>,
    pub shifts: Option<usize>,
}

impl Default for PerfHud {
    fn default() -> Self {
        Self::new()
    }
}

impl PerfHud {
    pub fn new() -> Self {
        Self {
            enabled: false,
            last_frame: None,
            samples: VecDeque::with_capacity(SAMPLE_COUNT),
        }
    }

    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(egui::Key::F12)) {
            self.enabled = !self.enabled;
        }

        let now = Instant::now();
        if let Some(last) = self.last_frame {
            if self.samples.len() == SAMPLE_COUNT {
                self.samples.pop_front();
            }
            self.samples.push_back((now - last).as_secs_f32() * 1000.0);
        }
        self.last_frame = Some(now);

        if self.enabled {
            ctx.request_repaint();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, info: &PerfInfo<'_>) {
        if !self.enabled {
            return;
        }

        let mut samples: Vec<f32> = self.samples.iter().copied().collect();
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let (avg, p95, worst) = if samples.is_empty() {
            (0.0, 0.0, 0.0)
        } else {
            let count = samples.len();
            let avg = samples.iter().sum::<f32>() / count as f32;
            let p95 = samples[(count * 95 / 100).min(count - 1)];
            let worst = samples.last().copied().unwrap_or(0.0);
            (avg, p95, worst)
        };
        let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };
        let frame_colour = frame_colour(avg);

        egui::Window::new("performance_hud")
            .anchor(Align2::RIGHT_TOP, vec2(-16.0, 76.0))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::surface().gamma_multiply(0.96))
                    .stroke(Stroke::new(1.0, theme::border()))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(12)),
            )
            .show(ctx, |ui| {
                ui.set_width(220.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("PERFORMANCE")
                            .color(theme::accent())
                            .font(theme::strong_font(10.0)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("F12").color(theme::faint()).size(9.0));
                    });
                });
                ui.add_space(6.0);

                row(ui, "Frame", &format!("{avg:.2} ms"), frame_colour);
                row(ui, "FPS", &format!("{fps:.0}"), frame_colour);
                row(
                    ui,
                    "p95 / worst",
                    &format!("{p95:.1} / {worst:.1} ms"),
                    theme::muted(),
                );
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                row(
                    ui,
                    "Cache",
                    if info.cache_ready {
                        "ready"
                    } else {
                        "starting"
                    },
                    if info.cache_ready {
                        theme::success()
                    } else {
                        theme::warning()
                    },
                );
                row(
                    ui,
                    "Contacts",
                    &format!("{} / {}", info.contacts_loaded, info.contacts_total),
                    theme::text(),
                );
                row(
                    ui,
                    "Total contacts",
                    &info.contacts_all.to_string(),
                    theme::muted(),
                );
                row(
                    ui,
                    "Last query",
                    &info
                        .last_query_ms
                        .map(|ms| format!("{ms:.2} ms"))
                        .unwrap_or_else(|| "—".to_owned()),
                    theme::muted(),
                );
                if let Some(shifts) = info.shifts {
                    row(ui, "Shifts", &shifts.to_string(), theme::muted());
                }

                if let Some(path) = info.cache_path {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(path)
                            .color(theme::faint())
                            .size(8.5)
                            .monospace(),
                    );
                }
                if let Some(error) = info.cache_error {
                    ui.add_space(4.0);
                    ui.label(RichText::new(error).color(theme::danger()).size(9.5));
                }
            });
    }
}

fn row(ui: &mut egui::Ui, label: &str, value: &str, colour: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(theme::muted()).size(10.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).color(colour).size(10.0).monospace());
        });
    });
}

fn frame_colour(avg_ms: f32) -> Color32 {
    if avg_ms <= 8.5 {
        theme::success()
    } else if avg_ms <= 16.7 {
        theme::warning()
    } else {
        theme::danger()
    }
}

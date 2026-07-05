//! Probes the region-highlight overlay viewport for the Wayland crash
//! (broken pipe / protocol error → WinitEventLoop(ExitFailure(1))).
//!
//! COMBO env var selects builder flags on top of
//! fullscreen+transparent+passthrough+undecorated:
//!   "TI" — always_on_top + active(false)   (the combination the app used)
//!   "T"  — always_on_top only
//!   "I"  — active(false) only
//!   ""   — neither
//!
//! Overlay shows 2s-6s, app exits at 8s. Exit 0 = combo is safe.

use std::io::Write;
use std::time::{Duration, Instant};

fn log(start: Instant, msg: &str) {
    println!("[{:6}ms] {}", start.elapsed().as_millis(), msg);
    let _ = std::io::stdout().flush();
}

struct Probe {
    start: Instant,
    last_log: Option<Instant>,
    combo: String,
}

impl eframe::App for Probe {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t = self.start.elapsed();

        if self
            .last_log
            .is_none_or(|l| l.elapsed() > Duration::from_millis(500))
        {
            self.last_log = Some(Instant::now());
            log(self.start, "parent alive");
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label(format!("highlight probe, combo={:?}", self.combo));
        });

        if t > Duration::from_secs(2) && t < Duration::from_secs(6) {
            let mut builder = egui::ViewportBuilder::default()
                .with_title("Region")
                .with_decorations(false)
                .with_transparent(true)
                .with_mouse_passthrough(true)
                .with_resizable(false)
                .with_fullscreen(true);
            if self.combo.contains('T') {
                builder = builder.with_always_on_top();
            }
            if self.combo.contains('I') {
                builder = builder.with_active(false);
            }
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("probe_highlight"),
                builder,
                |ctx, _class| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            let r = egui::Rect::from_min_size(
                                egui::pos2(300.0, 300.0),
                                egui::vec2(400.0, 250.0),
                            );
                            ui.painter().rect_stroke(
                                r,
                                0.0,
                                egui::Stroke::new(2.0, egui::Color32::WHITE),
                                egui::StrokeKind::Outside,
                            );
                        });
                },
            );
        }

        if t > Duration::from_secs(8) {
            log(self.start, "done — closing");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        ctx.request_repaint_after(Duration::from_millis(50));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        vsync: false,
        viewport: egui::ViewportBuilder::default().with_inner_size([288.0, 108.0]),
        ..Default::default()
    };
    let start = Instant::now();
    let combo = std::env::var("COMBO").unwrap_or_default();
    log(start, &format!("starting, combo={combo:?}"));
    eframe::run_native(
        "Highlight Probe",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(Probe {
                start,
                last_log: None,
                combo,
            }))
        }),
    )
}

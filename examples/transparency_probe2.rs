//! Decisive Wayland test, two questions in one run:
//!  1. Does a fullscreen transparent egui window actually render
//!     transparent (desktop visible through it), or black?
//!     → screenshot at 3s while root is fullscreen+transparent.
//!  2. Does enabling mouse passthrough make mutter revoke fullscreen?
//!     → passthrough flips on at 4.5s; screenshot 2 at 7s; check
//!       xdg_toplevel configure sizes in WAYLAND_DEBUG output.

use std::io::Write;
use std::time::{Duration, Instant};

const OUT1: &str = "/tmp/claude-1000/-home-ibbi-source-heartkelp/a258e695-8115-4ca3-a6fd-2c05b4d1c4e6/scratchpad/tp2_before_passthrough.png";
const OUT2: &str = "/tmp/claude-1000/-home-ibbi-source-heartkelp/a258e695-8115-4ca3-a6fd-2c05b4d1c4e6/scratchpad/tp2_after_passthrough.png";

fn shoot(rt: &tokio::runtime::Runtime, out: &str) {
    let result = rt.block_on(async {
        let response = ashpd::desktop::screenshot::Screenshot::request()
            .interactive(false)
            .send()
            .await?
            .response()?;
        response.uri().to_file_path().map_err(|_| {
            let e: Box<dyn std::error::Error + Send + Sync> = "bad uri".into();
            e
        })
    });
    match result {
        Ok(path) => {
            std::fs::copy(&path, out).expect("copy screenshot");
            let _ = std::fs::remove_file(&path);
            println!("SAVED {out}");
        }
        Err(e) => println!("FAILED {e}"),
    }
    let _ = std::io::stdout().flush();
}

struct Probe {
    start: Instant,
    passthrough_sent: bool,
}

impl eframe::App for Probe {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t = self.start.elapsed();

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                // A red frame near the middle; everything else should be
                // see-through if transparency works.
                ui.painter().rect_stroke(
                    egui::Rect::from_min_size(
                        egui::pos2(600.0, 300.0),
                        egui::vec2(400.0, 250.0),
                    ),
                    0.0,
                    egui::Stroke::new(3.0, egui::Color32::RED),
                    egui::StrokeKind::Outside,
                );
            });

        if t > Duration::from_millis(4500) && !self.passthrough_sent {
            self.passthrough_sent = true;
            println!("ENABLING PASSTHROUGH");
            let _ = std::io::stdout().flush();
            ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        }

        if t > Duration::from_secs(9) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        ctx.request_repaint_after(Duration::from_millis(50));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }
}

fn main() -> eframe::Result<()> {
    std::thread::spawn(|| {
        let rt = tokio::runtime::Runtime::new().expect("tokio");
        std::thread::sleep(Duration::from_secs(3));
        shoot(&rt, OUT1);
        std::thread::sleep(Duration::from_secs(4));
        shoot(&rt, OUT2);
    });

    let options = eframe::NativeOptions {
        vsync: false,
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_fullscreen(true),
        ..Default::default()
    };
    let start = Instant::now();
    eframe::run_native(
        "Transparency Probe 2",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(Probe {
                start,
                passthrough_sent: false,
            }))
        }),
    )
}

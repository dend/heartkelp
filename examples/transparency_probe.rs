//! Verifies viewport transparency on this machine (GNOME Wayland + NVIDIA,
//! glow renderer, vsync off).
//!
//! Opens the root window (transparent, red box painted in it) plus an
//! immediate fullscreen transparent click-through child viewport (green
//! dashed frame at 600,300 400x250 — same builder as the region
//! highlight), waits 3s, takes a portal screenshot of the whole screen,
//! saves it to the scratchpad, and exits. Inspect the PNG: if the child
//! viewport's background is black instead of see-through, child-viewport
//! transparency is broken.

use std::io::Write;
use std::time::Duration;

const OUT: &str = "/tmp/claude-1000/-home-ibbi-source-heartkelp/a258e695-8115-4ca3-a6fd-2c05b4d1c4e6/scratchpad/transparency_test.png";
const OUT2: &str = "/tmp/claude-1000/-home-ibbi-source-heartkelp/a258e695-8115-4ca3-a6fd-2c05b4d1c4e6/scratchpad/transparency_test2.png";

struct Probe;

impl eframe::App for Probe {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(80.0, 80.0)),
                    0.0,
                    egui::Color32::RED,
                );
            });

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("probe_highlight"),
            egui::ViewportBuilder::default()
                .with_title("Region")
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_mouse_passthrough(true)
                .with_resizable(false)
                .with_active(false)
                .with_fullscreen(true),
            |ctx, _class| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        let r = egui::Rect::from_min_size(
                            egui::pos2(600.0, 300.0),
                            egui::vec2(400.0, 250.0),
                        );
                        ui.painter().rect_stroke(
                            r,
                            0.0,
                            egui::Stroke::new(3.0, egui::Color32::GREEN),
                            egui::StrokeKind::Outside,
                        );
                    });
            },
        );

        ctx.request_repaint_after(Duration::from_millis(50));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }
}

fn main() -> eframe::Result<()> {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(3));
        let rt = tokio::runtime::Runtime::new().expect("tokio");
        let result = rt.block_on(async {
            let response = ashpd::desktop::screenshot::Screenshot::request()
                .interactive(false)
                .send()
                .await?
                .response()?;
            response
                .uri()
                .to_file_path()
                .map_err(|_| "bad uri".into())
        });
        match result {
            Ok(path) => {
                std::fs::copy(&path, OUT).expect("copy screenshot");
                let _ = std::fs::remove_file(&path);
                println!("SCREENSHOT_SAVED {OUT}");
            }
            Err(e) => {
                let e: Box<dyn std::error::Error> = e;
                println!("SCREENSHOT_FAILED {e}");
            }
        }
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_secs(5));
        let result2 = rt.block_on(async {
            let response = ashpd::desktop::screenshot::Screenshot::request()
                .interactive(false)
                .send()
                .await?
                .response()?;
            response
                .uri()
                .to_file_path()
                .map_err(|_| {
                    let e: Box<dyn std::error::Error + Send + Sync> = "bad uri".into();
                    e
                })
        });
        if let Ok(path) = result2 {
            std::fs::copy(&path, OUT2).expect("copy screenshot 2");
            let _ = std::fs::remove_file(&path);
            println!("SCREENSHOT2_SAVED {OUT2}");
        }
        let _ = std::io::stdout().flush();
        std::thread::sleep(Duration::from_millis(300));
        std::process::exit(0);
    });

    let options = eframe::NativeOptions {
        vsync: false,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([288.0, 108.0])
            .with_decorations(false)
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        "Transparency Probe",
        options,
        Box::new(|_cc| Ok(Box::new(Probe))),
    )
}

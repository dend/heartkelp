//! Verifies the raw-Wayland shm highlight overlay (src/highlight.rs):
//!  1. True transparency (software ARGB buffer, bypassing EGL/Vulkan)
//!  2. Dashes at the exact region coordinates (600,300 400x250)
//!  3. Fullscreen retained after another window takes focus
//!
//! Spawns the overlay, then an eframe window that steals focus, then
//! takes a portal screenshot at 4s. Exits by itself.

#[path = "../src/highlight.rs"]
mod highlight;

use std::io::Write;
use std::time::Duration;

const OUT: &str = "/tmp/claude-1000/-home-ibbi-source-heartkelp/a258e695-8115-4ca3-a6fd-2c05b4d1c4e6/scratchpad/shm_overlay.png";

struct Probe;

impl eframe::App for Probe {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("focus stealer window");
        });
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

fn main() -> eframe::Result<()> {
    let overlay = highlight::Highlight::spawn((600, 300, 400, 250));

    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(4));
        let rt = tokio::runtime::Runtime::new().expect("tokio");
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
                std::fs::copy(&path, OUT).expect("copy screenshot");
                let _ = std::fs::remove_file(&path);
                println!("SAVED {OUT}");
            }
            Err(e) => println!("FAILED {e}"),
        }
        let _ = std::io::stdout().flush();
        std::process::exit(0);
    });

    let options = eframe::NativeOptions {
        vsync: false,
        viewport: egui::ViewportBuilder::default().with_inner_size([288.0, 108.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "SHM Overlay Probe",
        options,
        Box::new(|_cc| Ok(Box::new(Probe))),
    );
    overlay.stop();
    result
}

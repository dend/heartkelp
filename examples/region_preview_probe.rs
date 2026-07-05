//! Drives the real RegionSelector with a preloaded region and verifies
//! visually (portal screenshot) that the selection rectangle appears at
//! the right screen position despite the fullscreen-configure race.
//!
//! Expected: white dashed rectangle at physical (600,300) size 400x250
//! on a solid dark-blue fullscreen overlay. Exits by itself.

#[path = "../src/region.rs"]
mod region;

use std::io::Write;
use std::time::Duration;

const OUT: &str = "/tmp/claude-1000/-home-ibbi-source-heartkelp/a258e695-8115-4ca3-a6fd-2c05b4d1c4e6/scratchpad/region_preview.png";

struct Probe {
    selector: region::RegionSelector,
}

impl eframe::App for Probe {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("region preview probe");
        });
        self.selector.show(ctx);
        ctx.request_repaint_after(Duration::from_millis(50));
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
        std::thread::sleep(Duration::from_millis(300));
        std::process::exit(0);
    });

    let options = eframe::NativeOptions {
        vsync: false,
        viewport: egui::ViewportBuilder::default().with_inner_size([288.0, 108.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Region Preview Probe",
        options,
        Box::new(|_cc| {
            let mut selector = region::RegionSelector::new();
            // Match the primary monitor so coordinates map 1:1
            let (w, h) = (2560usize, 1440usize);
            let pixels = vec![egui::Color32::from_rgb(20, 30, 80); w * h];
            let screenshot = egui::ColorImage::new([w, h], pixels);
            selector.set_initial_region(Some((600, 300, 400, 250)));
            selector.set_screenshot(screenshot);
            Ok(Box::new(Probe { selector }))
        }),
    )
}

//! Diagnostic probe for the Wayland viewport freeze (and its fix).
//!
//! History: immediate viewports froze the whole app on GNOME Wayland when
//! the parent window stopped getting frame callbacks (fullscreen overlay
//! occluding it, or the parent being minimized). The fix renders the
//! region-selector overlay as a *deferred* viewport that closes itself.
//!
//! This probe verifies that mechanism:
//!   0-3s   baseline: parent heartbeats
//!   3-7s   fullscreen deferred overlay; overlay logs its own heartbeats
//!          (parent heartbeats MAY pause while occluded — that's expected)
//!   7s     overlay closes itself from inside and wakes the parent
//!   7-12s  parent heartbeats must RESUME
//!   12s    clean exit
//!
//! Run: cargo run --example freeze_probe
//! Pass criteria: overlay heartbeats continue 3-7s, parent heartbeats
//! resume after 7s, process exits by itself with code 0.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn log(start: Instant, msg: &str) {
    println!("[{:6}ms] {}", start.elapsed().as_millis(), msg);
    let _ = std::io::stdout().flush();
}

#[derive(Default)]
struct Overlay {
    open: bool,
    frames: u64,
    last_log: Option<Instant>,
}

impl Overlay {
    fn ui(&mut self, ctx: &egui::Context, start: Instant) {
        if !self.open {
            return;
        }
        self.frames += 1;
        if self
            .last_log
            .is_none_or(|l| l.elapsed() > Duration::from_millis(250))
        {
            self.last_log = Some(Instant::now());
            log(start, &format!("  overlay frame #{}", self.frames));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| {
                ui.label("Heartkelp diagnostic overlay — closes by itself");
            });
        });
        ctx.request_repaint();

        if start.elapsed() > Duration::from_secs(7) {
            log(start, "  overlay closing itself, waking parent");
            self.open = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }
}

struct Probe {
    start: Instant,
    updates: u64,
    last_log: Option<Instant>,
    opened: bool,
    overlay: Arc<Mutex<Overlay>>,
}

impl eframe::App for Probe {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.updates += 1;
        let t = self.start.elapsed();

        if self
            .last_log
            .is_none_or(|l| l.elapsed() > Duration::from_millis(250))
        {
            self.last_log = Some(Instant::now());
            log(self.start, &format!("parent update #{}", self.updates));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("freeze probe (deferred-viewport fix verification)");
        });

        if t > Duration::from_secs(3) && !self.opened {
            self.opened = true;
            log(self.start, "opening fullscreen deferred overlay");
            self.overlay.lock().unwrap().open = true;
        }

        if self.overlay.lock().unwrap().open {
            let shared = Arc::clone(&self.overlay);
            let start = self.start;
            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of("probe_overlay"),
                egui::ViewportBuilder::default()
                    .with_title("Probe Overlay")
                    .with_fullscreen(true)
                    .with_decorations(false),
                move |ctx, _class| {
                    shared.lock().unwrap().ui(ctx, start);
                },
            );
        }

        if t > Duration::from_secs(12) {
            log(self.start, "done — closing");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        ctx.request_repaint();
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        // Same as the real app: NVIDIA EGL on Wayland blocks vsync'd
        // SwapBuffers forever for occluded surfaces.
        vsync: false,
        viewport: egui::ViewportBuilder::default().with_inner_size([288.0, 108.0]),
        ..Default::default()
    };
    let start = Instant::now();
    log(start, "starting probe");
    eframe::run_native(
        "Heartkelp Freeze Probe",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(Probe {
                start,
                updates: 0,
                last_log: None,
                opened: false,
                overlay: Arc::new(Mutex::new(Overlay::default())),
            }))
        }),
    )
}

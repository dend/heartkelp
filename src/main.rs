mod app;
mod capture;
mod config;
mod encoder;
mod highlight;
mod region;
mod types;

use std::sync::mpsc;

fn main() -> eframe::Result<()> {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();

    let options = eframe::NativeOptions {
        // NVIDIA's EGL on Wayland blocks SwapBuffers indefinitely for
        // occluded surfaces when vsync is on, freezing the event loop the
        // moment the fullscreen region overlay covers this window (see
        // examples/freeze_probe.rs). Visible windows are still throttled
        // by Wayland frame callbacks, so this doesn't cause a repaint spin.
        vsync: false,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([288.0, 108.0])
            .with_min_inner_size([268.0, 78.0])
            .with_resizable(false)
            .with_decorations(false)
            .with_always_on_top()
            .with_active(true)
            .with_transparent(true),
        ..Default::default()
    };

    // We need the egui context to pass to the backend thread.
    // eframe gives us the context inside run_native, so we spawn the backend
    // after the app is created, passing the context via a channel.
    let (ctx_tx, ctx_rx) = mpsc::channel::<egui::Context>();

    std::thread::spawn(move || {
        let ctx = ctx_rx.recv().expect("Failed to receive egui context");
        capture::run_backend(cmd_rx, event_tx, ctx);
    });

    eframe::run_native(
        "Heartkelp",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            // With vsync off, egui's built-in animations (tooltip fades)
            // request unthrottled repaints and spin at 1000+ fps. Disable
            // them; the app's hover effects are hand-painted anyway.
            cc.egui_ctx.style_mut(|s| s.animation_time = 0.0);
            let _ = ctx_tx.send(cc.egui_ctx.clone());
            Ok(Box::new(app::App::new(cmd_tx, event_rx)))
        }),
    )
}

mod app;
mod capture;
mod config;
mod encoder;
mod region;
mod types;

use std::sync::mpsc;

fn main() -> eframe::Result<()> {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();

    let options = eframe::NativeOptions {
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
            let _ = ctx_tx.send(cc.egui_ctx.clone());
            Ok(Box::new(app::App::new(cmd_tx, event_rx)))
        }),
    )
}

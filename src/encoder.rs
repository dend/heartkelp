use std::fs::File;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use gifski::{Repeat, Settings};
use imgref::ImgVec;
use rgb::RGBA8;

use crate::types::{Event, Frame};

pub fn encode_frames(
    frames: Vec<Frame>,
    fps: u8,
    start: usize,
    end: usize,
    width: Option<u32>,
    height: Option<u32>,
    output: PathBuf,
    event_tx: Sender<Event>,
    ctx: egui::Context,
) {
    let settings = Settings {
        width,
        height,
        quality: 100,
        fast: false,
        repeat: Repeat::Infinite,
    };

    let (collector, writer) = gifski::new(settings).expect("Failed to create gifski encoder");

    let output_clone = output.clone();
    let writer_handle = std::thread::spawn(move || {
        if let Some(parent) = output_clone.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = File::create(&output_clone).expect("Failed to create output file");
        writer.write(file, &mut gifski::progress::NoProgress {})
    });

    let frame_delay = 1.0 / fps as f64;

    for (i, frame) in frames[start..end].iter().enumerate() {
        let pixels: Vec<RGBA8> = frame
            .data
            .chunks_exact(4)
            .map(|c| RGBA8::new(c[0], c[1], c[2], c[3]))
            .collect();

        let img = ImgVec::new(pixels, frame.width as usize, frame.height as usize);
        let timestamp = i as f64 * frame_delay;

        if collector.add_frame_rgba(i, img, timestamp).is_err() {
            break;
        }
        let _ = event_tx.send(Event::EncodingProgress(i + 1));
        ctx.request_repaint();
    }

    drop(collector);

    match writer_handle.join() {
        Ok(Ok(())) => {
            let _ = event_tx.send(Event::RecordingFinished(output));
        }
        Ok(Err(e)) => {
            let _ = event_tx.send(Event::Error(format!("GIF write error: {e}")));
        }
        Err(_) => {
            let _ = event_tx.send(Event::Error("Writer thread panicked".into()));
        }
    }
    ctx.request_repaint();
}

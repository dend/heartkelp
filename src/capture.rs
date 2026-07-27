use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use crate::types::{CaptureMode, Command, Event, Frame};

pub fn run_backend(cmd_rx: Receiver<Command>, event_tx: Sender<Event>, ctx: egui::Context) {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Command::TakeScreenshot => {
                let tx = event_tx.clone();
                let ctx2 = ctx.clone();
                rt.block_on(async {
                    match take_screenshot().await {
                        Ok(image) => {
                            let _ = tx.send(Event::ScreenshotReady(image));
                        }
                        Err(e) => {
                            let _ = tx.send(Event::Error(format!("Screenshot failed: {e}")));
                        }
                    }
                    ctx2.request_repaint();
                });
            }
            Command::StartRecording { mode, fps } => {
                let tx = event_tx.clone();
                let ctx2 = ctx.clone();
                rt.block_on(async {
                    if let Err(e) =
                        start_recording(mode, fps, tx.clone(), ctx2.clone(), &cmd_rx).await
                    {
                        let _ = tx.send(Event::Error(format!("Recording failed: {e}")));
                        ctx2.request_repaint();
                    }
                });
            }
            Command::EncodeFrames {
                frames,
                fps,
                start,
                end,
                width,
                height,
                output_path,
            } => {
                let tx = event_tx.clone();
                let ctx2 = ctx.clone();
                std::thread::spawn(move || {
                    crate::encoder::encode_frames(
                        frames,
                        fps,
                        start,
                        end,
                        width,
                        height,
                        output_path,
                        tx,
                        ctx2,
                    );
                });
            }
            Command::StopRecording
            | Command::PauseRecording
            | Command::ResumeRecording => {
                // Handled inside start_recording via cmd_rx
            }
        }
    }
}

async fn take_screenshot() -> Result<egui::ColorImage, Box<dyn std::error::Error>> {
    let response = ashpd::desktop::screenshot::Screenshot::request()
        .interactive(false)
        .send()
        .await?
        .response()?;

    let uri = response.uri();
    let path = uri
        .to_file_path()
        .map_err(|_| "Invalid file URI from screenshot portal")?;

    let img = image::open(&path)?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let pixels = img
        .pixels()
        .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
        .collect();

    // Clean up the temp screenshot file
    let _ = std::fs::remove_file(&path);

    Ok(egui::ColorImage::new(size, pixels))
}

async fn start_recording(
    mode: CaptureMode,
    fps: u8,
    event_tx: Sender<Event>,
    ctx: egui::Context,
    cmd_rx: &Receiver<Command>,
) -> Result<(), Box<dyn std::error::Error>> {
    use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
    use ashpd::desktop::PersistMode;

    let proxy = Screencast::new().await?;
    let session = proxy.create_session().await?;

    proxy
        .select_sources(
            &session,
            CursorMode::Embedded,
            SourceType::Monitor.into(),
            false,
            None,
            PersistMode::DoNot,
        )
        .await?
        .response()?;

    let response = proxy.start(&session, None).await?.response()?;
    let streams = response.streams();

    if streams.is_empty() {
        return Err("No streams returned from portal".into());
    }

    let stream_info = &streams[0];
    let node_id = stream_info.pipe_wire_node_id();
    let pw_fd = proxy.open_pipe_wire_remote(&session).await?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let pause_flag = Arc::new(AtomicBool::new(false));

    // Channel for frames from PipeWire → collector
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<Frame>(8);

    // Spawn collector thread — accumulates frames for review
    // Sends RecordingStarted on first frame so the UI timer aligns
    // with actual capture rather than PipeWire startup.
    let collector_event_tx = event_tx.clone();
    let collector_ctx = ctx.clone();
    let collector_handle = std::thread::spawn(move || {
        let mut frames = Vec::new();
        let mut index = 0usize;
        while let Ok(mut frame) = frame_rx.recv() {
            // Frames arrive as raw BGRx rows; swizzle to RGBA here so the
            // PipeWire process callback stays cheap. The x byte is
            // undefined, so force alpha opaque.
            for px in frame.data.chunks_exact_mut(4) {
                px.swap(0, 2);
                px[3] = 255;
            }
            if index == 0 {
                let _ = collector_event_tx.send(Event::RecordingStarted);
            }
            frames.push(frame);
            index += 1;
            let _ = collector_event_tx.send(Event::FrameCaptured(index));
            collector_ctx.request_repaint();
        }
        frames
    });

    // Spawn PipeWire thread
    let pw_stop = stop_flag.clone();
    let pw_pause = pause_flag.clone();
    let pw_mode = mode.clone();
    let pw_handle = std::thread::spawn(move || {
        run_pipewire_capture(pw_fd, node_id, pw_mode, fps, frame_tx, pw_stop, pw_pause);
    });

    // Wait for StopRecording command
    loop {
        match cmd_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(Command::StopRecording) => {
                stop_flag.store(true, Ordering::SeqCst);
                break;
            }
            Ok(Command::PauseRecording) => {
                pause_flag.store(true, Ordering::SeqCst);
            }
            Ok(Command::ResumeRecording) => {
                pause_flag.store(false, Ordering::SeqCst);
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                stop_flag.store(true, Ordering::SeqCst);
                break;
            }
        }
    }

    let _ = pw_handle.join();

    match collector_handle.join() {
        Ok(frames) => {
            if let (Some(first), Some(last)) = (frames.first(), frames.last()) {
                let span = last.pts - first.pts;
                if span > 0.0 && frames.len() > 1 {
                    eprintln!(
                        "heartkelp: captured {} frames over {:.2}s (avg {:.1} fps, target {})",
                        frames.len(),
                        span,
                        (frames.len() - 1) as f64 / span,
                        fps
                    );
                }
            }
            let _ = event_tx.send(Event::RecordingReady { frames, fps });
            ctx.request_repaint();
        }
        Err(_) => {
            let _ = event_tx.send(Event::Error("Collector thread panicked".into()));
            ctx.request_repaint();
        }
    }

    Ok(())
}

fn run_pipewire_capture(
    fd: std::os::fd::OwnedFd,
    node_id: u32,
    mode: CaptureMode,
    fps: u8,
    frame_tx: std::sync::mpsc::SyncSender<Frame>,
    stop_flag: Arc<AtomicBool>,
    pause_flag: Arc<AtomicBool>,
) {
    pipewire::init();

    let mainloop =
        pipewire::main_loop::MainLoopBox::new(None).expect("Failed to create PW MainLoop");
    let context = pipewire::context::ContextBox::new(mainloop.loop_(), None)
        .expect("Failed to create PW Context");
    let core = context
        .connect_fd(fd, None)
        .expect("Failed to connect PW core via fd");

    let stream = pipewire::stream::StreamBox::new(
        &core,
        "heartkelp-capture",
        pipewire::properties::properties! {
            *pipewire::keys::MEDIA_TYPE => "Video",
            *pipewire::keys::MEDIA_CATEGORY => "Capture",
            *pipewire::keys::MEDIA_ROLE => "Screen",
        },
    )
    .expect("Failed to create PW stream");

    let stop2 = stop_flag.clone();
    let pause2 = pause_flag.clone();
    let mode2 = mode.clone();
    // Decimate delivery (at monitor refresh; see the maxFramerate comment on
    // the format pod) down to the target FPS by scheduled slots: a frame is
    // accepted if it lands within tolerance of the next slot, and the slot
    // advances by exactly one interval per accepted frame, so the average
    // rate locks to the target on any refresh rate. The tolerance (¼
    // interval) accepts frames that arrive one refresh tick early; without
    // it, "slightly early" frames get discarded and against tick-quantized
    // delivery that aliasing drops the rate to a subharmonic — the same bug
    // mutter's own throttle has (measured: maxFramerate 30/1 on a 59.95 Hz
    // monitor delivers a steady 20 fps). ¼ interval stays safely below the
    // half-interval that would let two consecutive refresh ticks through.
    let interval = 1.0 / fps as f64;
    let tolerance = interval * 0.25;
    let mut next_slot = 0.0_f64;
    let mut dropped = 0usize;
    let record_start = std::time::Instant::now();

    // Negotiated frame dimensions, written by param_changed and read by
    // process via the listener's shared user data.
    #[derive(Default)]
    struct StreamData {
        width: u32,
        height: u32,
    }

    let _listener = stream
        .add_local_listener::<StreamData>()
        .state_changed(
            |_stream: &pipewire::stream::Stream, _data: &mut StreamData, _old, new| {
                if let pipewire::stream::StreamState::Error(ref e) = new {
                    eprintln!("PipeWire stream error: {e}");
                }
            },
        )
        .param_changed(|_stream, negotiated: &mut StreamData, id, param| {
            let Some(param) = param else { return };
            if id != pipewire::spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) =
                pipewire::spa::param::format_utils::parse_format(param)
            else {
                return;
            };
            if media_type != pipewire::spa::param::format::MediaType::Video
                || media_subtype != pipewire::spa::param::format::MediaSubtype::Raw
            {
                return;
            }
            let mut info = pipewire::spa::param::video::VideoInfoRaw::new();
            if info.parse(param).is_ok() {
                negotiated.width = info.size().width;
                negotiated.height = info.size().height;
                let fr = info.framerate();
                let max = info.max_framerate();
                eprintln!(
                    "heartkelp: negotiated {}x{}, framerate {}/{}, maxFramerate {}/{}",
                    negotiated.width, negotiated.height, fr.num, fr.denom, max.num, max.denom
                );
            }
        })
        .process(move |stream: &pipewire::stream::Stream, negotiated: &mut StreamData| {
            if stop2.load(Ordering::SeqCst) {
                return;
            }

            // Always dequeue — dropping the Buffer requeues it to PipeWire,
            // so every early return below still recycles the buffer.
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };

            if pause2.load(Ordering::SeqCst) {
                return;
            }

            let pts = record_start.elapsed().as_secs_f64();
            if pts < next_slot - tolerance {
                return;
            }

            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else { return };
            let chunk = data.chunk();
            let chunk_stride = chunk.stride();
            let chunk_size = chunk.size() as usize;
            if chunk_stride <= 0 || chunk_size == 0 {
                return;
            }
            let stride = chunk_stride as usize;

            // Prefer negotiated dimensions; the stride-derived fallback
            // miscounts when rows are padded, so it is only used if the
            // Format param has somehow not arrived before buffers.
            let (width, height) = if negotiated.width > 0 && negotiated.height > 0 {
                (negotiated.width as usize, negotiated.height as usize)
            } else {
                let w = stride / 4;
                (w, if w > 0 { chunk_size / stride } else { 0 })
            };
            if width == 0 || height == 0 {
                return;
            }

            let Some(slice) = data.data() else { return };

            let (crop_x, crop_y, out_w, out_h) = match &mode2 {
                CaptureMode::FullScreen => (0usize, 0usize, width, height),
                CaptureMode::Region { x, y, w, h } => {
                    let rx = (*x).max(0) as usize;
                    let ry = (*y).max(0) as usize;
                    let rw = (*w as usize).min(width.saturating_sub(rx));
                    let rh = (*h as usize).min(height.saturating_sub(ry));
                    (rx, ry, rw, rh)
                }
            };
            if out_w == 0 || out_h == 0 {
                return;
            }

            // Copy out raw BGRx rows (dropping stride padding, cropping in
            // place for region mode). This callback must stay cheap — while
            // it runs, the compositor cannot recycle buffers and starts
            // dropping frames at the source — so the per-pixel RGBA swizzle
            // happens on the collector thread instead.
            let mut bgra = Vec::with_capacity(out_w * out_h * 4);
            for row in 0..out_h {
                let src_start = (crop_y + row) * stride + crop_x * 4;
                let src_end = src_start + out_w * 4;
                if src_end > slice.len() {
                    return;
                }
                bgra.extend_from_slice(&slice[src_start..src_end]);
            }

            let frame = Frame {
                data: bgra,
                width: out_w as u32,
                height: out_h as u32,
                pts,
            };

            match frame_tx.try_send(frame) {
                Ok(()) => {
                    next_slot += interval;
                    // More than a slot behind (startup, stall, pause) —
                    // re-anchor instead of burst-accepting to catch up.
                    if next_slot < pts {
                        next_slot = pts + interval;
                    }
                }
                Err(_) => {
                    // Collector behind; leaving the slot unchanged lets the
                    // next delivered frame retry immediately.
                    dropped += 1;
                    eprintln!("heartkelp: collector busy, dropped frame ({dropped} total)");
                }
            }
        })
        .register()
        .expect("Failed to register PW stream listener");

    // Build format parameters for BGRx video
    let obj = pipewire::spa::pod::Value::Object(pipewire::spa::pod::Object {
        type_: pipewire::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pipewire::spa::param::ParamType::EnumFormat.as_raw(),
        properties: vec![
            pipewire::spa::pod::Property {
                key: pipewire::spa::param::format::FormatProperties::MediaType.as_raw(),
                flags: pipewire::spa::pod::PropertyFlags::empty(),
                value: pipewire::spa::pod::Value::Id(pipewire::spa::utils::Id(
                    pipewire::spa::param::format::MediaType::Video.as_raw(),
                )),
            },
            pipewire::spa::pod::Property {
                key: pipewire::spa::param::format::FormatProperties::MediaSubtype.as_raw(),
                flags: pipewire::spa::pod::PropertyFlags::empty(),
                value: pipewire::spa::pod::Value::Id(pipewire::spa::utils::Id(
                    pipewire::spa::param::format::MediaSubtype::Raw.as_raw(),
                )),
            },
            pipewire::spa::pod::Property {
                key: pipewire::spa::param::format::FormatProperties::VideoFormat.as_raw(),
                flags: pipewire::spa::pod::PropertyFlags::empty(),
                value: pipewire::spa::pod::Value::Id(pipewire::spa::utils::Id(
                    pipewire::spa::param::video::VideoFormat::BGRx.as_raw(),
                )),
            },
            // framerate 0/1 declares a variable-rate stream; maxFramerate is
            // left effectively uncapped so mutter delivers at monitor
            // refresh and OUR slot decimator sets the pace. Do NOT request
            // the target FPS here: mutter's throttle aliases against the
            // refresh tick (measured on GNOME 46 @ 59.95 Hz: requesting
            // 30/1 yields a steady 20 fps), and frames it withholds are
            // unrecoverable client-side.
            pipewire::spa::pod::Property {
                key: pipewire::spa::param::format::FormatProperties::VideoFramerate.as_raw(),
                flags: pipewire::spa::pod::PropertyFlags::empty(),
                value: pipewire::spa::pod::Value::Fraction(pipewire::spa::utils::Fraction {
                    num: 0,
                    denom: 1,
                }),
            },
            pipewire::spa::pod::Property {
                key: pipewire::spa::param::format::FormatProperties::VideoMaxFramerate.as_raw(),
                flags: pipewire::spa::pod::PropertyFlags::empty(),
                value: pipewire::spa::pod::Value::Choice(
                    pipewire::spa::pod::ChoiceValue::Fraction(pipewire::spa::utils::Choice(
                        pipewire::spa::utils::ChoiceFlags::empty(),
                        pipewire::spa::utils::ChoiceEnum::Range {
                            default: pipewire::spa::utils::Fraction {
                                num: 1000,
                                denom: 1,
                            },
                            min: pipewire::spa::utils::Fraction { num: 1, denom: 1 },
                            max: pipewire::spa::utils::Fraction {
                                num: 1000,
                                denom: 1,
                            },
                        },
                    )),
                ),
            },
        ],
    });

    let param_bytes = pipewire::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &obj,
    )
    .expect("Failed to serialize PW params")
    .0
    .into_inner();

    // SAFETY: PodSerializer produces valid spa_pod-formatted data. Pod is
    // repr(transparent) over spa_pod. The bytes live on the stack and remain
    // valid through the connect call.
    let pod = unsafe { &*(param_bytes.as_ptr() as *const pipewire::spa::pod::Pod) };
    let mut params = [pod];

    stream
        .connect(
            pipewire::spa::utils::Direction::Input,
            Some(node_id),
            pipewire::stream::StreamFlags::AUTOCONNECT
                | pipewire::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .expect("Failed to connect PW stream");

    // Check stop flag periodically via timer and quit mainloop when set.
    // SAFETY: mainloop lives on this stack frame and won't be dropped until
    // after run() returns. The timer callback executes on the same thread
    // within run(). The pointer remains valid for the entire callback lifetime.
    let mainloop_ptr = &*mainloop as *const pipewire::main_loop::MainLoop;
    let stop_check = stop_flag.clone();

    let timer = mainloop.loop_().add_timer(move |_| {
        if stop_check.load(Ordering::SeqCst) {
            unsafe {
                (*mainloop_ptr).quit();
            }
        }
    });

    timer.update_timer(
        Some(std::time::Duration::from_millis(100)),
        Some(std::time::Duration::from_millis(100)),
    );

    mainloop.run();

    drop(timer);
    drop(_listener);
    drop(stream);
}

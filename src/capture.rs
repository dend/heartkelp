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
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<Frame>(4);

    // Spawn collector thread — accumulates frames for review
    // Sends RecordingStarted on first frame so the UI timer aligns
    // with actual capture rather than PipeWire startup.
    let collector_event_tx = event_tx.clone();
    let collector_ctx = ctx.clone();
    let collector_handle = std::thread::spawn(move || {
        let mut frames = Vec::new();
        let mut index = 0usize;
        while let Ok(frame) = frame_rx.recv() {
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
    let frame_interval = std::time::Duration::from_secs_f64(1.0 / fps as f64);
    let mut next_frame_time = std::time::Instant::now();

    let _listener = stream
        .add_local_listener::<()>()
        .state_changed(|_stream: &pipewire::stream::Stream, _data: &mut (), _old, new| {
            if let pipewire::stream::StreamState::Error(ref e) = new {
                eprintln!("PipeWire stream error: {e}");
            }
        })
        .process(move |stream: &pipewire::stream::Stream, _data: &mut ()| {
            if stop2.load(Ordering::SeqCst) {
                return;
            }

            // Dequeue and discard frames while paused
            if pause2.load(Ordering::SeqCst) {
                let _ = stream.dequeue_buffer();
                return;
            }

            // Throttle to configured FPS — advance from scheduled
            // time (not wall clock) to prevent drift accumulation.
            let now = std::time::Instant::now();
            if now < next_frame_time {
                let _ = stream.dequeue_buffer();
                return;
            }
            next_frame_time += frame_interval;
            // If we fell far behind (e.g. stall), reset to avoid burst
            if next_frame_time < now {
                next_frame_time = now + frame_interval;
            }

            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if let Some(data) = datas.first_mut() {
                    let chunk = data.chunk();
                    let stride = chunk.stride();
                    if stride <= 0 {
                        return;
                    }
                    let stride = stride as u32;
                    let width = stride / 4;
                    let height = if width > 0 {
                        chunk.size() / stride
                    } else {
                        0
                    };

                    if width == 0 || height == 0 {
                        return;
                    }

                    if let Some(slice) = data.data() {
                        // Convert BGRA to RGBA
                        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                        for y in 0..height {
                            let row_start = (y * stride) as usize;
                            for x in 0..width {
                                let offset = row_start + (x * 4) as usize;
                                if offset + 3 < slice.len() {
                                    let b = slice[offset];
                                    let g = slice[offset + 1];
                                    let r = slice[offset + 2];
                                    let a = slice[offset + 3];
                                    rgba.extend_from_slice(&[r, g, b, a]);
                                }
                            }
                        }

                        // Crop if region mode
                        let frame = match &mode2 {
                            CaptureMode::FullScreen => Frame {
                                data: rgba,
                                width,
                                height,
                            },
                            CaptureMode::Region {
                                x: rx,
                                y: ry,
                                w: rw,
                                h: rh,
                            } => {
                                let rx = (*rx).max(0) as u32;
                                let ry = (*ry).max(0) as u32;
                                let rw = (*rw).min(width.saturating_sub(rx));
                                let rh = (*rh).min(height.saturating_sub(ry));

                                if rw == 0 || rh == 0 {
                                    return;
                                }

                                let mut cropped = Vec::with_capacity((rw * rh * 4) as usize);
                                for cy in 0..rh {
                                    let src_y = ry + cy;
                                    let src_start = (src_y * width + rx) as usize * 4;
                                    let src_end = src_start + (rw as usize * 4);
                                    if src_end <= rgba.len() {
                                        cropped.extend_from_slice(&rgba[src_start..src_end]);
                                    }
                                }

                                Frame {
                                    data: cropped,
                                    width: rw,
                                    height: rh,
                                }
                            }
                        };

                        // Non-blocking send — drop frame if encoder is behind
                        let _ = frame_tx.try_send(frame);
                    }
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

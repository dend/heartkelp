# Working with Heartkelp

## Build & Run

```bash
# System dependencies (Ubuntu/Debian)
sudo apt install build-essential libpipewire-0.3-dev libclang-dev pkg-config

# Build
cargo build

# Run
cargo run
```

There are no tests yet. Verify changes compile with `cargo build`.

## Project Structure

```
src/
  main.rs       — Entry point. Sets up mpsc channels, spawns backend thread, launches eframe.
  types.rs      — Shared types: Command, Event, RecordingState, CaptureMode, Frame.
  app.rs        — All GUI code. The App struct, state machine, and custom-painted UI.
  capture.rs    — Backend thread. Handles ashpd portal calls, PipeWire frame capture, encoding dispatch.
  encoder.rs    — GIF encoding via gifski. Runs on a separate thread.
  config.rs     — Config struct (FPS, output dir). Persisted as TOML in ~/.config/heartkelp/.
  region.rs     — Fullscreen overlay for dragging a region selection rectangle.
```

## Architecture

The app uses a command/event architecture over `std::sync::mpsc` channels:

- **GUI thread** (`app.rs`) sends `Command` variants to the backend and processes `Event` variants each frame.
- **Backend thread** (`capture.rs`) runs a tokio runtime, handles portal calls via ashpd, manages PipeWire streams, and dispatches encoding.
- **Encoder thread** (`encoder.rs`) receives frames and produces a GIF file, sending progress events back.

State machine in `app.rs`: `Idle → SelectingRegion → Recording → Encoding → Reviewing → Idle`

The `Encoding` state is used briefly after stopping a recording (waiting for frames to be ready). GIF encoding from the review editor happens inline — the app stays in `Reviewing` state with `encoding_in_progress` tracking the encode.

## GUI Patterns

The UI is entirely custom-painted using egui's painter API — no standard widgets. Key patterns:

- **Design tokens** — constants at the top of `app.rs` for colors, sizes, spacing, and fonts.
- **Button pattern** — `allocate_exact_size` → `interact` → paint background rect → paint label text → handle click.
- **Centering** — compute total content width, add `(avail - total) / 2` padding at the start of a `ui.horizontal`.
- **Window sizing** — `send_viewport_cmd(InnerSize(...))` called each frame based on visible content.
- **Child viewports** — settings uses `show_viewport_immediate`; the region selector uses `show_viewport_deferred` with `Arc<Mutex>` state (its fullscreen overlay occludes the main window, and an immediate viewport would freeze with it on Wayland). The recording controls are NOT a child viewport — the main window becomes the controls bar during recording.

### Wayland constraints (verified on GNOME + NVIDIA; see `examples/freeze_probe.rs`, `examples/transparency_probe2.rs`)
- **Window transparency does not work** on this stack: eframe glow + NVIDIA EGL renders "transparent" surfaces as opaque black (verified by screenshot probe). Never design UI relying on a see-through window. This is why there is no dashed region-highlight overlay and why the recording controls live in the main window.
- **Window positioning is ignored** (`with_position` does nothing on Wayland) — never place a window at screen coordinates; draw at coordinates inside a window instead, or don't.
- Mutter revokes fullscreen from windows created with `with_active(false)` (~40 ms after granting it). Enabling mouse passthrough on a focused window is fine.
- Never minimize/hide the main window while a child viewport must stay live — occluded/unmapped surfaces stop getting frame callbacks, which stalls immediate viewports.
- vsync is off (`main.rs`) because NVIDIA's EGL blocks `SwapBuffers` indefinitely for occluded surfaces. Consequences: never call bare `request_repaint()` in a per-frame loop (use `request_repaint_after(...)`); egui animations are disabled (`animation_time = 0`) and `ui.spinner()` is banned — both request unthrottled repaints; a ~144 fps sleep cap guards `App::update` and the region overlay as the backstop.
- Send viewport commands (`InnerSize`, `MousePassthrough`, …) only when the value changes (`set_window_size`, `sent_passthrough`). Each command schedules a repaint, so unconditional per-frame sends create a repaint loop that floods the Wayland socket until GNOME silently disconnects the app (broken pipe, exit 1, no protocol error).
- A deferred child viewport must close itself (`ViewportCommand::Close` + `request_repaint_of(ROOT)`) because its occluded parent may be paused and unable to retire it.

## Key API Notes

### pipewire-rs 0.9
- Use `MainLoopBox::new()`, `ContextBox::new()`, `StreamBox::new()`.
- `ContextBox::new()` takes `(&Loop, Option<PropertiesBox>)` — pass `mainloop.loop_()`.
- Stream callbacks: `state_changed` takes 4 params, `process` takes 2.

### egui 0.33
- `ColorImage::new(size, pixels)` — no struct literal construction.
- `painter.rect_stroke()` takes 4 args — 4th is `StrokeKind`.

### ashpd 0.12
- `PersistMode` is in `ashpd::desktop`, not `ashpd::desktop::screencast`.
- `SourceType::Monitor.into()` for `BitFlags<SourceType>`.
- `select_sources().await?.response()?` — needs `.response()`.

## Common Tasks

### Adding a new UI element to the review editor
Edit `show_review_editing_ui()` in `app.rs`. Follow the existing allocate → paint → interact pattern. Update `REVIEW_CONTROLS_H` or the dynamic `status_h` if vertical space changes.

### Adding a new command/event
1. Add the variant to `Command` or `Event` in `types.rs`.
2. Handle sending in `app.rs` and receiving in `capture.rs` (or vice versa).
3. Call `ctx.request_repaint()` after sending events so the GUI picks them up.

### Changing default settings
Edit `Config::default()` in `config.rs`. The config file is at `~/.config/heartkelp/config.toml`.

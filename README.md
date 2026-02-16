![Heartkelp image, with Kril from Another Crab's Treasure](media/kelp.webp)

# 💚 Heartkelp

A Wayland screen-to-GIF recorder for Linux. Record your full screen or a selected region, trim the result, and save as an animated GIF.

## Dependencies

### System packages (Ubuntu/Debian)

```bash
sudo apt install build-essential libpipewire-0.3-dev libclang-dev pkg-config
```

### Rust

Requires Rust 1.85+ (edition 2024). Install via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Building

```bash
cargo build --release
```

The binary will be at `target/release/heartkelp`.

## Running

```bash
cargo run --release
```

## Usage

### Recording

1. Launch the app. A small floating window appears.
2. Choose a capture mode:
   - **Full** — records the entire monitor.
   - **Region** — records a rectangular area. Click the crop button to take a screenshot and drag a selection on the overlay. A minimap with dimensions shows the selected area.
3. Click the red record button. Your compositor will show a portal dialog asking to share the screen — accept it.
4. The app minimizes and a floating control bar appears near the capture area with:
   - A timer and frame counter.
   - **Pause/Resume** — temporarily halt capture.
   - **Stop** — end the recording.
5. After stopping, the app processes captured frames and opens the review editor.

### Reviewing and trimming

The review editor shows a preview of the recording with playback controls and a timeline.

- **Play/Pause** — toggle playback of the trimmed region.
- **Timeline** — click or drag to scrub the playhead.
- **Trim handles** — drag the orange bracket handles `[` `]` on the timeline to set the start and end points. Areas outside the trim range are dimmed.
- **Time ruler** — shows timestamps below the timeline for reference.

### Saving

- Click **Save** to encode the trimmed frames as a GIF. Encoding progress is shown inline below the buttons while the editor stays fully interactive.
- Once saved, a status line appears with the file path and a **Show in Folder** button.
- You can adjust the trim handles (which clears the saved status) and save again.
- Click **Close** to return to the idle screen. If you haven't saved yet, a confirmation dialog asks whether to save first.

### Settings

Click the gear icon to open settings:

- **Frames Per Second** — default capture FPS (1-30). Applies to the next recording.
- **Output Directory** — where GIFs are saved. Browse to change.

Settings are stored in `~/.config/heartkelp/config.toml`.

GIF files are named `heartkelp_YYYY-MM-DD_HH-MM-SS.gif` using UTC time.

## Architecture

```
egui GUI ──commands──> Backend thread (tokio + ashpd portals)
                              │
                       PipeWire fd + node_id
                              │
                       PipeWire thread (frame capture)
                              │
                         frames (mpsc)
                              │
                       Encoder thread (gifski)
                              │
                           output.gif
```

- **ashpd** — XDG Desktop Portal calls for screenshots and screencasting.
- **PipeWire** — captures frames from the compositor.
- **gifski** — encodes high-quality animated GIFs.
- **egui/eframe** — immediate-mode GUI with custom-painted controls.

## Requirements

- A Wayland compositor with XDG Desktop Portal support (GNOME, KDE Plasma, Sway, etc.)
- X11 is not supported
- The portal dialog is a security feature — it lets you choose which monitor/window to share

## License

AGPL-3.0-or-later

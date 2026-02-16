# heartkelp

A Wayland screen-to-GIF recorder for Linux. Select a region or record the full screen, then save it as an animated GIF.

## Dependencies

### System packages (Ubuntu/Debian)

```bash
sudo apt install build-essential libpipewire-0.3-dev libclang-dev pkg-config
```

### Rust

Requires Rust 1.85+ (edition 2024). Install via [rustup](https://rustup.rs/) if you don't have it:

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

A GUI window will open with the following controls:

- **Select Region** — takes a screenshot, opens a fullscreen overlay where you drag to select a rectangle
- **Record Full Screen** — records the entire monitor
- **Record Region** — records the previously selected region
- **FPS** slider — frame rate for the GIF (1-30, default 10)
- **Output** — path for the output GIF file
- **Stop** — stops recording and encodes the GIF

### Typical workflow

1. Launch the app
2. (Optional) Click **Select Region** and drag a rectangle on the overlay
3. Set the desired FPS and output path
4. Click **Record Full Screen** or **Record Region**
5. Your compositor will show a portal dialog asking to share the screen — accept it
6. Click **Stop** when done
7. Wait for encoding to finish — the output path will be shown when complete

## How it works

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

- **ashpd** handles the XDG Desktop Portal calls for screenshots and screencasting
- **PipeWire** captures frames from the compositor
- **gifski** encodes high-quality animated GIFs

## Notes

- Requires a Wayland compositor with XDG Desktop Portal support (GNOME, KDE Plasma, Sway, etc.)
- X11 is not supported
- The portal dialog is a security feature — it lets you choose which monitor to share

## License

AGPL-3.0-or-later

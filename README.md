![Heartkelp image, with Kril from Another Crab's Treasure](media/kelp.webp)

# 💚 Heartkelp

A Wayland-compatible screen-to-GIF recorder for Linux (primarily developed and tested on Ubuntu). Record your full screen or a selected region, trim the result, and save as an animated GIF.

>[!NOTE]
>This application is **very much experimental**. There might be things that aren't working properly - if you find something like this, [open an issue](https://github.com/dend/heartkelp/issues).

## Dependencies

You will need these if you want to build the application locally.

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

From the root project folder, run:

```bash
cargo build --release
```

The binary will be at `target/release/heartkelp`.

## Running

Also from the root project folder, run:

```bash
cargo run --release
```

## Requirements

- A Wayland compositor with XDG Desktop Portal support (GNOME, KDE Plasma, Sway)
- X11 is **not supported**

## Image credit

The image in this README is not the project logo — it's Kril, the hermit crab protagonist of [Another Crab's Treasure](https://aggrocrab.com/ACT) by Aggro Crab. It's a fantastic game and this is my small homage to it.

## License

AGPL-3.0-or-later

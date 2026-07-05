//! On-screen region highlight: a fullscreen, click-through overlay with a
//! dashed border drawn around the selected region.
//!
//! This deliberately does NOT use eframe/winit: GPU-rendered window
//! transparency is broken on this stack (NVIDIA EGL and Vulkan both
//! composite "transparent" as opaque black). Wayland compositors do,
//! however, alpha-composite software ARGB8888 `wl_shm` buffers correctly,
//! so this module speaks raw Wayland on a dedicated thread and draws the
//! overlay with the CPU.
//!
//! Known platform limitation: GNOME has no client-side "always on top",
//! so windows the user raises later will cover the overlay.

use std::os::fd::{AsRawFd, BorrowedFd};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_region, wl_registry, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{
    delegate_noop, Connection, Dispatch, QueueHandle,
};
use wayland_protocols::xdg::shell::client::{
    xdg_surface, xdg_toplevel, xdg_wm_base,
};

/// Handle to a running highlight overlay. Dropping it (or calling
/// `stop()`) removes the overlay.
pub struct Highlight {
    stop_tx: Sender<()>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Highlight {
    /// Show a dashed border around `region` (physical pixels). The border
    /// is drawn just OUTSIDE the region bounds so region captures never
    /// contain it.
    pub fn spawn(region: (i32, i32, u32, u32)) -> Highlight {
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let join = std::thread::Builder::new()
            .name("region-highlight".into())
            .spawn(move || {
                if let Err(e) = run(region, stop_rx) {
                    eprintln!("Region highlight overlay failed: {e}");
                }
            })
            .ok();
        Highlight { stop_tx, join }
    }

    #[allow(dead_code)] // the app relies on Drop; probes call stop() explicitly
    pub fn stop(mut self) {
        let _ = self.stop_tx.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for Highlight {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct State {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    surface: Option<wl_surface::WlSurface>,
    region: (i32, i32, u32, u32),
    size: (u32, u32),
    running: bool,
    needs_draw: bool,
}

/// A maximized window's origin is the work-area origin (below the GNOME
/// top bar, beside/above any fixed dock), not (0,0). Mutter mirrors the
/// work area to Xwayland's `_NET_WORKAREA`, so query it there to convert
/// global screen coordinates into overlay-local ones.
fn workarea_origin() -> (i32, i32) {
    fn query() -> Option<(i32, i32)> {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen_num)?.root;
        let atom = conn
            .intern_atom(false, b"_NET_WORKAREA")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let prop = conn
            .get_property(false, root, atom, AtomEnum::CARDINAL, 0, 4)
            .ok()?
            .reply()
            .ok()?;
        let vals: Vec<u32> = prop.value32()?.collect();
        Some((*vals.first()? as i32, *vals.get(1)? as i32))
    }
    query().unwrap_or((0, 0))
}

fn run(
    region: (i32, i32, u32, u32),
    stop_rx: Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (wa_x, wa_y) = workarea_origin();
    let region = (region.0 - wa_x, region.1 - wa_y, region.2, region.3);

    let conn = Connection::connect_to_env()?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    conn.display().get_registry(&qh, ());

    let mut state = State {
        compositor: None,
        shm: None,
        wm_base: None,
        surface: None,
        region,
        size: (0, 0),
        running: true,
        needs_draw: false,
    };

    // Collect globals
    event_queue.roundtrip(&mut state)?;

    let compositor = state
        .compositor
        .clone()
        .ok_or("no wl_compositor global")?;
    let wm_base = state.wm_base.clone().ok_or("no xdg_wm_base global")?;
    state.shm.as_ref().ok_or("no wl_shm global")?;

    let surface = compositor.create_surface(&qh, ());
    let xdg_surface = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg_surface.get_toplevel(&qh, ());
    toplevel.set_title("Heartkelp Region".into());
    toplevel.set_app_id("heartkelp-region-highlight".into());
    // Maximized, NOT fullscreen: mutter composites fullscreen windows over
    // an opaque black backdrop, which defeats ARGB transparency entirely.
    // A maximized window is composited normally over the desktop.
    toplevel.set_maximized();

    // Empty input region: all clicks pass through the overlay.
    let input_region = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&input_region));
    input_region.destroy();

    state.surface = Some(surface.clone());
    surface.commit();

    // Event loop: dispatch Wayland events, watch for the stop signal.
    loop {
        event_queue.flush()?;

        if let Some(guard) = event_queue.prepare_read() {
            let fd = guard.connection_fd().as_raw_fd();
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let n = unsafe { libc::poll(&mut pfd, 1, 100) };
            if n > 0 {
                let _ = guard.read();
            }
            // n == 0: timeout — guard dropped, releasing the read lock
        }

        event_queue.dispatch_pending(&mut state)?;

        if state.needs_draw {
            state.needs_draw = false;
            draw_and_attach(&mut state, &qh)?;
        }

        match stop_rx.try_recv() {
            Ok(()) | Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }
        if !state.running {
            break;
        }
    }

    toplevel.destroy();
    xdg_surface.destroy();
    surface.destroy();
    event_queue.flush()?;
    Ok(())
}

fn draw_and_attach(
    state: &mut State,
    qh: &QueueHandle<State>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (w, h) = state.size;
    if w == 0 || h == 0 {
        return Ok(());
    }
    let (shm, surface) = match (&state.shm, &state.surface) {
        (Some(s), Some(su)) => (s.clone(), su.clone()),
        _ => return Ok(()),
    };

    let stride = w as usize * 4;
    let len = stride * h as usize;

    // Anonymous shared memory for the buffer
    let fd = unsafe {
        libc::memfd_create(c"heartkelp-highlight".as_ptr(), libc::MFD_CLOEXEC)
    };
    if fd < 0 {
        return Err("memfd_create failed".into());
    }
    if unsafe { libc::ftruncate(fd, len as libc::off_t) } < 0 {
        unsafe { libc::close(fd) };
        return Err("ftruncate failed".into());
    }
    let map = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    if map == libc::MAP_FAILED {
        unsafe { libc::close(fd) };
        return Err("mmap failed".into());
    }

    let pixels =
        unsafe { std::slice::from_raw_parts_mut(map as *mut u32, len / 4) };
    draw_overlay(pixels, w as usize, h as usize, state.region);

    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    let pool = shm.create_pool(borrowed, len as i32, qh, ());
    let buffer = pool.create_buffer(
        0,
        w as i32,
        h as i32,
        stride as i32,
        wl_shm::Format::Argb8888,
        qh,
        (),
    );
    pool.destroy();
    unsafe {
        libc::munmap(map, len);
        libc::close(fd);
    }

    surface.attach(Some(&buffer), 0, 0);
    surface.damage_buffer(0, 0, w as i32, h as i32);
    surface.commit();
    Ok(())
}

/// Draw the dashed border into a transparent ARGB8888 (premultiplied)
/// buffer. The border band sits 1..=6 px OUTSIDE the region bounds.
fn draw_overlay(pixels: &mut [u32], w: usize, h: usize, region: (i32, i32, u32, u32)) {
    pixels.fill(0); // fully transparent

    let (rx, ry, rw, rh) = region;
    let (rx, ry) = (rx as i64, ry as i64);
    let (rw, rh) = (rw as i64, rh as i64);

    const SHADOW: u32 = 0x6E000000; // premultiplied black, ~43% alpha
    const WHITE: u32 = 0xFFFFFFFF;
    const DASH_ON: i64 = 14;
    const DASH_PERIOD: i64 = 22;

    let mut put = |x: i64, y: i64, c: u32| {
        if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
            pixels[y as usize * w + x as usize] = c;
        }
    };

    // Contrast band: solid semi-dark, 1..=6 px outside the region
    for d in 1..=6i64 {
        for x in (rx - d)..(rx + rw + d) {
            put(x, ry - d, SHADOW);
            put(x, ry + rh - 1 + d, SHADOW);
        }
        for y in (ry - d)..(ry + rh + d) {
            put(rx - d, y, SHADOW);
            put(rx + rw - 1 + d, y, SHADOW);
        }
    }

    // White dashes: 3 px thick, 2..=4 px outside the region
    for d in 2..=4i64 {
        for x in (rx - d)..(rx + rw + d) {
            let phase = (x - rx).rem_euclid(DASH_PERIOD);
            if phase < DASH_ON {
                put(x, ry - d, WHITE);
                put(x, ry + rh - 1 + d, WHITE);
            }
        }
        for y in (ry - d)..(ry + rh + d) {
            let phase = (y - ry).rem_euclid(DASH_PERIOD);
            if phase < DASH_ON {
                put(rx - d, y, WHITE);
                put(rx + rw - 1 + d, y, WHITE);
            }
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(
                        name,
                        version.min(4),
                        qh,
                        (),
                    ));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind(name, 1, qh, ()));
                }
                "xdg_wm_base" => {
                    state.wm_base = Some(registry.bind(
                        name,
                        version.min(2),
                        qh,
                        (),
                    ));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(
        _: &mut Self,
        wm_base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for State {
    fn event(
        state: &mut Self,
        xdg_surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg_surface.ack_configure(serial);
            state.needs_draw = true;
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for State {
    fn event(
        state: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } => {
                if width > 0 && height > 0 {
                    state.size = (width as u32, height as u32);
                }
            }
            xdg_toplevel::Event::Close => {
                state.running = false;
            }
            _ => {}
        }
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_region::WlRegion);

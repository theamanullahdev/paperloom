//! Wayland: a `wlr-layer-shell` surface per widget on the *bottom* layer —
//! above the wallpaper (the background layer the engine draws on), below
//! every normal window — with the pixels in shared memory. Works on
//! compositors that offer the protocol (KDE Plasma, Sway, Hyprland, COSMIC,
//! Cinnamon's Wayland session); GNOME's Mutter does not.

use std::error::Error;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use tiny_skia::Pixmap;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_output, wl_shm, wl_surface};
use wayland_client::{Connection, QueueHandle};

use paperloom_common::widgets::{Placed, Widgets};

use crate::control::Control;
use crate::render::Text;
use crate::widgets::{self, Context};

struct Window {
    layer: LayerSurface,
    placed: Placed,
    /// The compositor has told us the surface is ready to receive content.
    configured: bool,
    pixmap: Option<Pixmap>,
    due: Instant,
    dirty: bool,
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    windows: Vec<Window>,
    exit: bool,
}

pub fn run(widgets: &Widgets, text: &mut Text, control: &mut Control) -> Result<(), Box<dyn Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).map_err(|err| format!("wl_compositor not available: {err}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|err| format!("this desktop doesn't offer wlr-layer-shell, so widgets can't be placed on it: {err}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|err| format!("wl_shm not available: {err}"))?;
    let pool = SlotPool::new(64 * 1024, &shm)?;

    let mut app = App { registry_state: RegistryState::new(&globals), output_state: OutputState::new(&globals, &qh), shm, pool, windows: Vec::new(), exit: false };
    // Output sizes arrive with the first round trips; widget positions are fractions of one.
    queue.roundtrip(&mut app)?;
    queue.roundtrip(&mut app)?;
    let (screen_w, screen_h) = app
        .output_state
        .outputs()
        .next()
        .and_then(|output| app.output_state.info(&output))
        .and_then(|info| info.logical_size)
        .unwrap_or((1920, 1080));

    for placed in widgets.placed.iter().cloned() {
        let (w, h) = widgets::size_of(&placed.kind);
        let (w, h) = ((w as f32 * placed.scale) as u32, (h as f32 * placed.scale) as u32);
        let surface = compositor.create_surface(&qh);
        let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Bottom, Some("paperloom-widget"), None);
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_margin((placed.y * screen_h as f32) as i32, 0, 0, (placed.x * screen_w as f32) as i32);
        layer.set_size(w, h);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        // Pointer input isn't handled here yet, so the surface stays click-through.
        {
            // An empty input region: every click goes to what is underneath.
            if let Ok(region) = Region::new(&compositor) {
                layer.wl_surface().set_input_region(Some(region.wl_region()));
            }
        }
        layer.commit();
        app.windows.push(Window { layer, placed, configured: false, pixmap: None, due: Instant::now(), dirty: true });
    }

    while !app.exit && !control.changed() {
        queue.dispatch_pending(&mut app)?;

        let now = Instant::now();
        let paused = control.paused();
        if !paused {
            control.refresh_sample();
        }
        let slow = control.slow();
        for index in 0..app.windows.len() {
            let window = &mut app.windows[index];
            if !window.configured || paused {
                continue;
            }
            if window.due <= now || window.pixmap.is_none() {
                match widgets::render(&window.placed.kind, window.placed.scale, text, &Context { sample: &control.sample, slow }) {
                    Some(frame) => {
                        window.due = now + frame.next_change;
                        window.pixmap = Some(frame.pixmap);
                        window.dirty = true;
                    }
                    None => window.due = now + Duration::from_secs(3600),
                }
            }
            if window.dirty {
                window.dirty = false;
                if let Some(pixmap) = window.pixmap.take() {
                    present(&mut app.pool, &app.windows[index].layer, &pixmap, app.windows[index].placed.opacity);
                    app.windows[index].pixmap = Some(pixmap);
                }
            }
        }
        queue.flush()?;

        // Sleep until the next widget is due or the compositor says something.
        let wake = app.windows.iter().map(|window| window.due).min().unwrap_or(now + Duration::from_secs(1));
        let nap = wake.saturating_duration_since(Instant::now()).min(Duration::from_secs(1));
        if let Some(guard) = queue.prepare_read() {
            let mut poll = libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // SAFETY: one valid pollfd, count 1.
            let ready = unsafe { libc::poll(&mut poll, 1, nap.as_millis() as i32 + 1) };
            if ready > 0 {
                guard.read()?;
            }
        }
    }
    Ok(())
}

/// Premultiplied RGBA → the compositor's ARGB8888 (BGRA bytes), scaled by `opacity`.
fn present(pool: &mut SlotPool, layer: &LayerSurface, pixmap: &Pixmap, opacity: f32) {
    let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
    let Ok((buffer, canvas)) = pool.create_buffer(width, height, width * 4, wl_shm::Format::Argb8888) else { return };
    let factor = (opacity.clamp(0.1, 1.0) * 256.0) as u32;
    for (out, source) in canvas.as_chunks_mut::<4>().0.iter_mut().zip(pixmap.data().as_chunks::<4>().0) {
        let scale = |value: u8| ((u32::from(value) * factor) >> 8) as u8;
        *out = [scale(source[2]), scale(source[1]), scale(source[0]), scale(source[3])];
    }
    let surface = layer.wl_surface();
    surface.damage_buffer(0, 0, width, height);
    if buffer.attach_to(surface).is_ok() {
        surface.commit();
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        if let Some(window) = self.windows.iter_mut().find(|window| window.layer.wl_surface() == layer.wl_surface()) {
            window.configured = true;
            window.dirty = true;
        }
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(App);

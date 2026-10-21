//! Wayland desktop-layer surface + libmpv video rendering, via
//! `wlr-layer-shell` and EGL.
//!
//! Unlike X11 — where libmpv can be handed a plain window id (`wid`) and
//! left to render into it internally — Wayland has no such shortcut.
//! Instead this: creates a background-layer surface spanning every output
//! (same real-estate idea as `backends::x11`'s desktop-type window), sets
//! up an EGL context against that surface, hands libmpv's *render API* an
//! OpenGL context via that EGL setup, and drives a render loop off the
//! surface's own frame callbacks — each time the compositor says it's
//! ready for the next frame, pull whatever libmpv has decoded and draw it.
//!
//! **This has never run against a live compositor.** This dev machine is
//! X11-only, so there is nowhere to actually test any of this — every
//! piece below (the EGL call sequence, the Mpv render-API wiring, the
//! non-blocking Wayland dispatch loop) was written against the real
//! installed source of `smithay-client-toolkit`, `khronos-egl`, and
//! `wayland-backend` (not guessed from memory), and it compiles clean, but
//! "compiles" and "correctly renders a frame on a real wlr-layer-shell
//! compositor" are different claims. Treat this as a first real attempt
//! that needs a live Wayland session to actually debug, not as verified
//! working code.

use std::error::Error;
use std::ffi::c_void;
use std::sync::Arc;
use std::time::{Duration, Instant};

use khronos_egl as egl;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
use libmpv2::Mpv;
use paperloom_common::ipc::Listener;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, FrameCallbackData};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_output, wl_seat, wl_surface};
use wayland_client::{backend::WaylandError, Connection, Proxy, QueueHandle};

/// The `GLContext` type mpv's render API hands back to our GL-function
/// loader callback. It's the EGL instance itself (shared via `Arc` with
/// the copy we keep for our own `make_current`/`swap_buffers` calls),
/// since resolving a GL function is just `eglGetProcAddress`.
type Egl = Arc<egl::DynamicInstance<egl::EGL1_4>>;

use crate::player::{self, Flow};

pub fn run(initial_path: Option<&str>) -> Result<(), Box<dyn Error>> {
    player::validate_cli_path(initial_path)?;

    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).map_err(|err| format!("wl_compositor not available: {err}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|err| format!("wlr-layer-shell not available: {err}"))?;

    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Background, Some("paperloom"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(0, 0);
    layer.commit();

    // --- EGL setup against this Wayland connection ---
    //
    // Loaded dynamically (dlopen "libEGL.so.1" at runtime) rather than
    // linked at build time, so this crate still builds without an EGL dev
    // package installed — consistent with everything else here only ever
    // being build-verified, never run.
    let egl = Arc::new(unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required() }?);
    let display_ptr = conn.backend().display_ptr() as *mut c_void;
    let egl_display = unsafe { egl.get_display(display_ptr) }.ok_or("eglGetDisplay failed")?;
    egl.initialize(egl_display)?;
    egl.bind_api(egl::OPENGL_API)?;

    let config_attribs = [egl::RED_SIZE, 8, egl::GREEN_SIZE, 8, egl::BLUE_SIZE, 8, egl::NONE];
    let egl_config = egl.choose_first_config(egl_display, &config_attribs)?.ok_or("no suitable EGL config")?;

    let context_attribs = [egl::CONTEXT_MAJOR_VERSION, 3, egl::CONTEXT_MINOR_VERSION, 0, egl::NONE];
    let egl_context = egl.create_context(egl_display, egl_config, None, &context_attribs)?;

    let mpv: &'static Mpv = Box::leak(Box::new(Mpv::new()?));
    player::configure_wallpaper(mpv)?;

    let mut listener = Listener::bind()?;

    let mut state = State {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        layer,
        egl,
        egl_display,
        egl_context,
        egl_config,
        egl_surface: None,
        display_ptr,
        mpv,
        render_ctx: None,
        wl_egl_surface: None,
        width: 0,
        height: 0,
        initial_path: player::startup_wallpaper(initial_path),
        is_paused: false,
        last_pause_check: Instant::now(),
        exit: false,
    };

    loop {
        event_queue.dispatch_pending(&mut state)?;

        if let Some(guard) = event_queue.prepare_read() {
            match guard.read() {
                Ok(_) => {
                    event_queue.dispatch_pending(&mut state)?;
                }
                Err(WaylandError::Io(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) => return Err(err.into()),
            }
        }
        event_queue.flush()?;

        if player::poll_ipc(&mut listener, state.mpv) == Flow::Quit {
            return Ok(());
        }

        if state.exit {
            return Ok(());
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

struct State {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    layer: LayerSurface,

    egl: Egl,
    egl_display: egl::Display,
    egl_context: egl::Context,
    egl_config: egl::Config,
    /// Created once, on the first configure — see `setup_rendering`. Every
    /// subsequent render/swap reuses this same surface; only its pixel
    /// size changes (via `wl_egl_surface.resize`), the EGL surface object
    /// itself doesn't need recreating for a resize.
    egl_surface: Option<egl::Surface>,
    display_ptr: *mut c_void,

    mpv: &'static Mpv,
    render_ctx: Option<RenderContext<'static>>,
    // Must outlive `egl_surface`, which is created from it — kept here so
    // it's never dropped while still in use, and so `configure` can call
    // `.resize()` on it for every configure after the first.
    wl_egl_surface: Option<wayland_egl::WlEglSurface>,

    width: u32,
    height: u32,
    initial_path: Option<String>,

    is_paused: bool,
    last_pause_check: Instant,

    exit: bool,
}

impl State {
    /// First-ever configure: create the EGL window surface, make it
    /// current, build mpv's render context against it, load the initial
    /// file if there was one, render the first frame, and kick off the
    /// frame-callback render loop.
    fn setup_rendering(&mut self, qh: &QueueHandle<Self>) -> Result<(), Box<dyn Error>> {
        let wl_egl_surface = wayland_egl::WlEglSurface::new(self.layer.wl_surface().id(), self.width as i32, self.height as i32)?;

        let egl_surface = unsafe {
            self.egl.create_window_surface(self.egl_display, self.egl_config, wl_egl_surface.ptr() as egl::NativeWindowType, None)?
        };
        self.egl.make_current(self.egl_display, Some(egl_surface), Some(egl_surface), Some(self.egl_context))?;
        self.egl_surface = Some(egl_surface);
        self.wl_egl_surface = Some(wl_egl_surface);

        fn get_proc_address(ctx: &Egl, name: &str) -> *mut c_void {
            ctx.get_proc_address(name).map_or(std::ptr::null_mut(), |f| f as *mut c_void)
        }

        let render_ctx = self.mpv.create_render_context([
            RenderParam::ApiType(RenderParamApiType::OpenGl),
            RenderParam::InitParams(OpenGLInitParams { get_proc_address, ctx: Arc::clone(&self.egl) }),
            RenderParam::WaylandDisplay(self.display_ptr),
        ])?;
        self.render_ctx = Some(render_ctx);

        if let Some(path) = self.initial_path.take() {
            player::play(self.mpv, &path);
        }

        self.render_current_frame();
        self.request_next_frame(qh);
        Ok(())
    }

    /// Renders whatever mpv currently has (a new frame if one's ready, the
    /// previous one again otherwise — always safe to call, per mpv's own
    /// render() docs) and presents it. No-op until `setup_rendering` has
    /// run once.
    fn render_current_frame(&mut self) {
        let (Some(render_ctx), Some(egl_surface)) = (&self.render_ctx, self.egl_surface) else {
            return;
        };
        // FBO 0 = this surface's own default framebuffer; mpv renders
        // directly into it, we never issue any GL draw calls ourselves.
        if let Err(err) = render_ctx.render::<Egl>(0, self.width as i32, self.height as i32, true) {
            eprintln!("paperloom-engine: mpv render failed: {err}");
            return;
        }
        if let Err(err) = self.egl.swap_buffers(self.egl_display, egl_surface) {
            eprintln!("paperloom-engine: eglSwapBuffers failed: {err}");
            return;
        }
        render_ctx.report_swap();
    }

    fn request_next_frame(&self, qh: &QueueHandle<Self>) {
        let surface = self.layer.wl_surface();
        surface.frame(qh, FrameCallbackData(surface.clone()));
    }

    fn maybe_update_pause_state(&mut self) {
        if self.last_pause_check.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_pause_check = Instant::now();

        let settings = paperloom_common::load();
        player::periodic(&settings, self.mpv);
        // Only the two platform-independent triggers apply here: there's
        // no portable Wayland way to ask "what's fullscreen" or "what's
        // covering the desktop" the way X11's EWMH properties allow (by
        // design — Wayland doesn't expose other clients' window state to
        // arbitrary clients). `pause_on_fullscreen_app` and
        // `pause_on_window_covering_desktop` simply have no effect here.
        let should_pause = (settings.pause_on_screen_lock && crate::pause::screen_is_locked())
            || (settings.pause_on_low_battery && crate::pause::battery_low(settings.low_battery_percent));

        if should_pause != self.is_paused {
            self.is_paused = should_pause;
            player::set_paused(self.mpv, self.is_paused);
        }
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.maybe_update_pause_state();
        self.render_current_frame();
        self.request_next_frame(qh);
    }

    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let first = self.width == 0 && self.height == 0;
        self.width = configure.new_size.0;
        self.height = configure.new_size.1;
        if self.width == 0 || self.height == 0 {
            return;
        }

        if first {
            if let Err(err) = self.setup_rendering(qh) {
                eprintln!("paperloom-engine: failed to set up Wayland rendering: {err}");
            }
        } else if let Some(wl_egl_surface) = &self.wl_egl_surface {
            wl_egl_surface.resize(self.width as i32, self.height as i32, 0, 0);
        }
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    // Deliberately never binding a keyboard/pointer: this surface never
    // wants input, same as X11's `input-default-bindings=false`.
    fn new_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, _: Capability) {}
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, _: Capability) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

delegate_registry!(State);

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(State);

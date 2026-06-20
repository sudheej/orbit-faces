use crate::events::{self, FaceEvent, FaceState};
use crate::hitmask::CircleHitMask;
use crate::lua_host::LuaHost;
use crate::renderer;
use anyhow::Context;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Instant;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

const BTN_LEFT: u32 = 0x110;
const INITIAL_MARGIN: i32 = 64;

pub fn run(face_dir: PathBuf) -> anyhow::Result<()> {
    let manifest_path = face_dir.join("manifest.json");
    let manifest = LuaHost::read_manifest(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let lua_host =
        LuaHost::load(face_dir.join(&manifest.script)).context("failed to load Lua face script")?;

    let connection =
        Connection::connect_to_env().context("failed to connect to the Wayland compositor")?;
    let (globals, mut event_queue) =
        registry_queue_init(&connection).context("failed to read Wayland globals")?;
    let qh = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).context("wl_compositor is not available")?;
    let layer_shell =
        LayerShell::bind(&globals, &qh).context("wlr-layer-shell is not available")?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm is not available")?;

    let width = manifest.window.width;
    let height = manifest.window.height;
    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Overlay,
        Some("orbital-face-host"),
        None,
    );
    layer.set_anchor(Anchor::TOP | Anchor::LEFT);
    layer.set_margin(INITIAL_MARGIN, 0, 0, INITIAL_MARGIN);
    layer.set_size(width, height);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);

    let hit_mask = CircleHitMask {
        width,
        height,
        radius: width.min(height) as f32 * 0.44,
    };
    set_circle_input_region(&compositor, &qh, &layer, hit_mask);
    layer.commit();

    let pool_size = width as usize * height as usize * 4 * 3;
    let pool = SlotPool::new(pool_size, &shm).context("failed to create Wayland SHM pool")?;
    let mut state = WaylandApp {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pointer: None,
        layer,
        pool,
        width,
        height,
        hit_mask,
        margin_left: INITIAL_MARGIN,
        margin_top: INITIAL_MARGIN,
        drag_position: None,
        configured: false,
        exit: false,
        error: None,
        lua_host,
        stdin_events: events::spawn_stdin_reader(),
        state: FaceState::Idle,
        started_at: Instant::now(),
        last_tick: Instant::now(),
    };
    state.lua_host.call_load()?;

    while !state.exit {
        event_queue
            .blocking_dispatch(&mut state)
            .context("Wayland event dispatch failed")?;
    }

    if let Some(error) = state.error {
        return Err(error);
    }
    Ok(())
}

struct WaylandApp {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pointer: Option<wl_pointer::WlPointer>,
    layer: LayerSurface,
    pool: SlotPool,
    width: u32,
    height: u32,
    hit_mask: CircleHitMask,
    margin_left: i32,
    margin_top: i32,
    drag_position: Option<(f64, f64)>,
    configured: bool,
    exit: bool,
    error: Option<anyhow::Error>,
    lua_host: LuaHost,
    stdin_events: Receiver<FaceEvent>,
    state: FaceState,
    started_at: Instant,
    last_tick: Instant,
}

impl WaylandApp {
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if let Err(error) = self.try_draw(qh) {
            self.error = Some(error);
            self.exit = true;
        }
    }

    fn try_draw(&mut self, qh: &QueueHandle<Self>) -> anyhow::Result<()> {
        while let Ok(event) = self.stdin_events.try_recv() {
            match event {
                FaceEvent::StateChanged(next_state) => {
                    self.state = next_state;
                    self.lua_host.call_state_changed(next_state)?;
                }
                FaceEvent::AlwaysOnTopChanged(enabled) => {
                    self.layer
                        .set_layer(if enabled { Layer::Overlay } else { Layer::Top });
                }
            }
        }

        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        self.lua_host.call_update(dt)?;
        let commands = self
            .lua_host
            .draw(self.state, self.started_at.elapsed().as_secs_f32())?;

        let stride = self.width as i32 * 4;
        let (buffer, canvas) = self
            .pool
            .create_buffer(
                self.width as i32,
                self.height as i32,
                stride,
                wl_shm::Format::Argb8888,
            )
            .context("failed to create Wayland frame buffer")?;
        renderer::render_argb8888(canvas, self.width, self.height, &commands);

        self.layer
            .wl_surface()
            .damage_buffer(0, 0, self.width as i32, self.height as i32);
        self.layer
            .wl_surface()
            .frame(qh, self.layer.wl_surface().clone());
        buffer
            .attach_to(self.layer.wl_surface())
            .context("failed to attach Wayland frame buffer")?;
        self.layer.commit();
        Ok(())
    }

    fn update_drag(&mut self, position: (f64, f64)) {
        let Some(previous) = self.drag_position.replace(position) else {
            return;
        };
        self.margin_left += (position.0 - previous.0).round() as i32;
        self.margin_top += (position.1 - previous.1).round() as i32;
        self.margin_left = self.margin_left.max(0);
        self.margin_top = self.margin_top.max(0);
        self.layer
            .set_margin(self.margin_top, 0, 0, self.margin_left);
        self.layer.commit();
    }
}

fn set_circle_input_region(
    compositor: &CompositorState,
    _qh: &QueueHandle<WaylandApp>,
    layer: &LayerSurface,
    hit_mask: CircleHitMask,
) {
    let region = Region::new(compositor).expect("failed to create Wayland input region");
    let center_x = hit_mask.width as f32 * 0.5;
    let center_y = hit_mask.height as f32 * 0.5;

    for y in 0..hit_mask.height {
        let dy = y as f32 + 0.5 - center_y;
        let squared_span = hit_mask.radius * hit_mask.radius - dy * dy;
        if squared_span <= 0.0 {
            continue;
        }
        let span = squared_span.sqrt();
        let left = (center_x - span).floor().max(0.0) as i32;
        let right = (center_x + span).ceil().min(hit_mask.width as f32) as i32;
        region.add(left, y as i32, right - left, 1);
    }

    layer.set_input_region(Some(region.wl_region()));
}

impl CompositorHandler for WaylandApp {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.draw(qh);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for WaylandApp {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        self.width = NonZeroU32::new(configure.new_size.0).map_or(self.width, NonZeroU32::get);
        self.height = NonZeroU32::new(configure.new_size.1).map_or(self.height, NonZeroU32::get);
        if !self.configured {
            self.configured = true;
            self.draw(qh);
        }
    }
}

impl SeatHandler for WaylandApp {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(error) => {
                    self.error = Some(anyhow::anyhow!("failed to create Wayland pointer: {error}"));
                    self.exit = true;
                }
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for WaylandApp {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.layer.wl_surface() {
                continue;
            }

            match event.kind {
                PointerEventKind::Press { button, .. }
                    if button == BTN_LEFT
                        && self
                            .hit_mask
                            .contains_xy(event.position.0 as f32, event.position.1 as f32) =>
                {
                    self.drag_position = Some(event.position);
                }
                PointerEventKind::Motion { .. } if self.drag_position.is_some() => {
                    self.update_drag(event.position);
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    self.drag_position = None;
                }
                PointerEventKind::Leave { .. } => {
                    self.drag_position = None;
                }
                _ => {}
            }
        }
    }
}

impl OutputHandler for WaylandApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for WaylandApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_compositor!(WaylandApp);
delegate_output!(WaylandApp);
delegate_shm!(WaylandApp);
delegate_seat!(WaylandApp);
delegate_pointer!(WaylandApp);
delegate_layer!(WaylandApp);
delegate_registry!(WaylandApp);

impl ProvidesRegistryState for WaylandApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

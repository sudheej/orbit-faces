use crate::bridge::{BridgeHandle, BridgeUpdate, FaceToRuntimeEvent};
use crate::events::{self, FaceEvent, RuntimeMessage};
use crate::face_pack::{FacePack, LaunchOptions};
use crate::hitmask::CircleHitMask;
use crate::renderer;
use crate::runtime::FaceRuntime;
use anyhow::Context;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
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
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

const BTN_LEFT: u32 = 0x110;
const INITIAL_MARGIN: i32 = 64;

pub fn run(options: LaunchOptions) -> anyhow::Result<()> {
    let pack = FacePack::load(options.face_dir)?;
    let status = format!(
        "t:{} b:{} shape:input click:os",
        if pack.manifest.window.transparent {
            "y"
        } else {
            "n"
        },
        if pack.manifest.window.borderless {
            "y"
        } else {
            "n"
        }
    );
    let mut runtime = FaceRuntime::load(pack, status)?;
    // A layer-shell companion must use Overlay to remain visible above normal
    // Hyprland windows. The manifest preference can still be toggled at runtime.
    runtime.always_on_top = true;
    let bridge = options.bridge_url.map(|url| {
        runtime.enable_bridge_mode();
        BridgeHandle::connect(
            url,
            FaceToRuntimeEvent::Ready {
                face: runtime.pack.manifest.name.clone(),
                version: runtime.pack.manifest.version.clone(),
            },
        )
    });

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

    let width = runtime.pack.manifest.window.width;
    let height = runtime.pack.manifest.window.height;
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
    layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);

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
        keyboard: None,
        layer,
        pool,
        width,
        height,
        hit_mask,
        margin_left: INITIAL_MARGIN,
        margin_top: INITIAL_MARGIN,
        drag_position: None,
        drag_moved: false,
        last_click: None,
        configured: false,
        exit: false,
        error: None,
        runtime,
        stdin_events: events::spawn_stdin_reader(),
        bridge,
        started_at: Instant::now(),
        last_tick: Instant::now(),
    };

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
    keyboard: Option<wl_keyboard::WlKeyboard>,
    layer: LayerSurface,
    pool: SlotPool,
    width: u32,
    height: u32,
    hit_mask: CircleHitMask,
    margin_left: i32,
    margin_top: i32,
    drag_position: Option<(f64, f64)>,
    drag_moved: bool,
    last_click: Option<Instant>,
    configured: bool,
    exit: bool,
    error: Option<anyhow::Error>,
    runtime: FaceRuntime,
    stdin_events: Receiver<FaceEvent>,
    bridge: Option<BridgeHandle>,
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
            self.apply_face_event(event);
        }
        while let Some(update) = self
            .bridge
            .as_ref()
            .and_then(|bridge| bridge.try_recv().ok())
        {
            self.apply_bridge_update(update);
        }

        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        let commands = self
            .runtime
            .update_and_draw(dt, self.started_at.elapsed().as_secs_f32());

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

    fn set_always_on_top(&mut self, enabled: bool) {
        self.runtime.always_on_top = enabled;
        self.layer
            .set_layer(if enabled { Layer::Overlay } else { Layer::Top });
        self.layer.commit();
    }

    fn update_drag(&mut self, position: (f64, f64)) {
        let Some(previous) = self.drag_position.replace(position) else {
            return;
        };
        let dx = (position.0 - previous.0).round() as i32;
        let dy = (position.1 - previous.1).round() as i32;
        if dx != 0 || dy != 0 {
            self.drag_moved = true;
        }
        self.margin_left += dx;
        self.margin_top += dy;
        self.margin_left = self.margin_left.max(0);
        self.margin_top = self.margin_top.max(0);
        self.layer
            .set_margin(self.margin_top, 0, 0, self.margin_left);
        self.layer.commit();
    }

    fn apply_face_event(&mut self, event: FaceEvent) {
        match event {
            FaceEvent::State(state_event) => self.runtime.change_state(state_event),
            FaceEvent::Config {
                always_on_top,
                debug_overlay,
            } => {
                if let Some(enabled) = always_on_top {
                    self.set_always_on_top(enabled);
                }
                if let Some(enabled) = debug_overlay {
                    self.runtime.debug = enabled;
                }
            }
        }
    }

    fn apply_bridge_update(&mut self, update: BridgeUpdate) {
        match update {
            BridgeUpdate::Connected => self.runtime.set_bridge_connected(true),
            BridgeUpdate::Disconnected => self.runtime.set_bridge_connected(false),
            BridgeUpdate::Sent(event_type) => self.runtime.record_sent_event(event_type),
            BridgeUpdate::Received(message) => match message {
                RuntimeMessage::Event { event_type, event } => {
                    self.runtime.record_received_event(event_type);
                    self.apply_face_event(event);
                }
                RuntimeMessage::Ping { .. } => self.runtime.record_received_event("ping"),
                RuntimeMessage::Unknown { event_type } => {
                    eprintln!("warning: ignored unknown bridge message type {event_type:?}");
                    self.runtime.record_received_event(event_type);
                }
            },
        }
    }

    fn send_bridge_event(&self, event: FaceToRuntimeEvent) {
        if let Some(bridge) = &self.bridge {
            bridge.send(event);
        }
    }

    fn finish_pointer_interaction(&mut self, position: (f64, f64)) {
        if self.drag_position.take().is_none() {
            return;
        }
        if self.drag_moved {
            self.send_bridge_event(FaceToRuntimeEvent::Dragged {
                x: self.margin_left,
                y: self.margin_top,
            });
        } else {
            let x = position.0 as f32;
            let y = position.1 as f32;
            self.send_bridge_event(FaceToRuntimeEvent::Clicked {
                x,
                y,
                button: "left".into(),
            });
            let now = Instant::now();
            if self
                .last_click
                .is_some_and(|previous| now.duration_since(previous) <= Duration::from_millis(500))
            {
                self.send_bridge_event(FaceToRuntimeEvent::DoubleClicked {
                    x,
                    y,
                    button: "left".into(),
                });
                self.send_bridge_event(FaceToRuntimeEvent::Action {
                    action: "toggle_listening".into(),
                });
                self.last_click = None;
            } else {
                self.last_click = Some(now);
            }
        }
        self.drag_moved = false;
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
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(keyboard) => self.keyboard = Some(keyboard),
                Err(error) => {
                    self.error = Some(anyhow::anyhow!(
                        "failed to create Wayland keyboard: {error}"
                    ));
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
        if capability == Capability::Keyboard {
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
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
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    let x = event.position.0 as f32;
                    let y = event.position.1 as f32;
                    if self
                        .runtime
                        .hit_test(x, y)
                        .unwrap_or_else(|| self.hit_mask.contains_xy(x, y))
                    {
                        self.drag_position = Some(event.position);
                        self.drag_moved = false;
                    }
                }
                PointerEventKind::Motion { .. } if self.drag_position.is_some() => {
                    self.update_drag(event.position);
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    self.finish_pointer_interaction(event.position);
                }
                PointerEventKind::Leave { .. } => {
                    self.drag_position = None;
                    self.drag_moved = false;
                }
                _ => {}
            }
        }
    }
}

impl KeyboardHandler for WaylandApp {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        match event.keysym {
            Keysym::a | Keysym::A => self.set_always_on_top(!self.runtime.always_on_top),
            Keysym::d | Keysym::D => self.runtime.debug = !self.runtime.debug,
            Keysym::Escape => self.exit = true,
            _ => {}
        }
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
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
delegate_keyboard!(WaylandApp);
delegate_pointer!(WaylandApp);
delegate_layer!(WaylandApp);
delegate_registry!(WaylandApp);

impl ProvidesRegistryState for WaylandApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

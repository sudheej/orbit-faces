use crate::bridge::{BridgeHandle, BridgeUpdate, FaceToRuntimeEvent};
use crate::events::{self, FaceEvent, RuntimeMessage};
use crate::face_pack::{FacePack, LaunchOptions};
use crate::platform;
use crate::renderer::{self, DrawCommand};
use crate::runtime::FaceRuntime;
use crate::window::{self, DragOutcome, HostWindow};
use anyhow::Context;
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

const FRAME_TIME: Duration = Duration::from_millis(16);

pub struct App {
    sdl: sdl3::Sdl,
    host_window: HostWindow,
    runtime: FaceRuntime,
    stdin_events: Receiver<FaceEvent>,
    bridge: Option<BridgeHandle>,
}

impl App {
    pub fn new(options: LaunchOptions) -> anyhow::Result<Self> {
        let pack = FacePack::load(options.face_dir)?;
        let sdl = sdl3::init().context("failed to initialize SDL3")?;
        let host_window = window::create(&sdl, &pack.manifest.window)
            .context("failed to create SDL3 host window")?;
        let mut runtime = FaceRuntime::load(pack, "trans:try shape:sdl click:try")?;
        let stdin_events = events::spawn_stdin_reader();
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

        Ok(Self {
            sdl,
            host_window,
            runtime,
            stdin_events,
            bridge,
        })
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let mut event_pump = self
            .sdl
            .event_pump()
            .context("failed to create SDL event pump")?;
        let start = Instant::now();
        let mut last_tick = start;

        'running: loop {
            let now = Instant::now();
            let dt = now.duration_since(last_tick).as_secs_f32();
            last_tick = now;

            for event in event_pump.poll_iter() {
                match event {
                    Event::Quit { .. }
                    | Event::KeyDown {
                        keycode: Some(Keycode::Escape),
                        ..
                    } => break 'running,
                    Event::KeyDown {
                        keycode: Some(Keycode::A),
                        ..
                    } => self.toggle_always_on_top(),
                    Event::KeyDown {
                        keycode: Some(Keycode::D),
                        ..
                    } => self.runtime.debug = !self.runtime.debug,
                    Event::MouseButtonDown {
                        mouse_btn: MouseButton::Left,
                        x,
                        y,
                        ..
                    } => {
                        self.host_window.begin_drag(x, y);
                    }
                    Event::MouseButtonUp {
                        mouse_btn: MouseButton::Left,
                        clicks,
                        x,
                        y,
                        ..
                    } => self.finish_pointer_interaction(x, y, clicks),
                    Event::MouseMotion { x, y, .. } => self.host_window.drag_to(x, y),
                    _ => {}
                }
            }

            while let Ok(face_event) = self.stdin_events.try_recv() {
                self.apply_face_event(face_event);
            }
            while let Some(update) = self
                .bridge
                .as_ref()
                .and_then(|bridge| bridge.try_recv().ok())
            {
                self.apply_bridge_update(update);
            }

            let commands = self
                .runtime
                .update_and_draw(dt, start.elapsed().as_secs_f32());
            self.render(commands);
            std::thread::sleep(FRAME_TIME);
        }

        Ok(())
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
                RuntimeMessage::SwitchFace { face } => {
                    self.runtime.record_received_event("face.switch");
                    self.switch_face(&face);
                }
                RuntimeMessage::Unknown { event_type } => {
                    eprintln!("warning: ignored unknown bridge message type {event_type:?}");
                    self.runtime.record_received_event(event_type);
                }
            },
        }
    }

    fn switch_face(&mut self, requested: &str) {
        let path = self.runtime.pack.resolve_switch_path(requested);
        let result = FacePack::load(path).and_then(|pack| {
            self.host_window.resize_for_face(&pack.manifest.window)?;
            self.runtime.switch_pack(pack)
        });
        match result {
            Ok(()) => {
                eprintln!("switched face to {:?}", self.runtime.pack.manifest.name);
                self.send_bridge_event(FaceToRuntimeEvent::Ready {
                    face: self.runtime.pack.manifest.name.clone(),
                    version: self.runtime.pack.manifest.version.clone(),
                });
            }
            Err(error) => eprintln!("face switch failed for {requested:?}: {error:#}"),
        }
    }

    fn finish_pointer_interaction(&mut self, x: f32, y: f32, clicks: u8) {
        match self.host_window.end_drag() {
            Some(DragOutcome::Click) => {
                self.send_bridge_event(FaceToRuntimeEvent::Clicked {
                    x,
                    y,
                    button: "left".into(),
                });
                if clicks >= 2 {
                    self.send_bridge_event(FaceToRuntimeEvent::DoubleClicked {
                        x,
                        y,
                        button: "left".into(),
                    });
                    self.send_bridge_event(FaceToRuntimeEvent::Action {
                        action: "toggle_listening".into(),
                    });
                }
            }
            Some(DragOutcome::Dragged { x, y }) => {
                self.send_bridge_event(FaceToRuntimeEvent::Dragged { x, y });
            }
            None => {}
        }
    }

    fn send_bridge_event(&self, event: FaceToRuntimeEvent) {
        if let Some(bridge) = &self.bridge {
            bridge.send(event);
        }
    }

    fn toggle_always_on_top(&mut self) {
        self.set_always_on_top(!self.runtime.always_on_top);
    }

    fn set_always_on_top(&mut self, enabled: bool) {
        if let Err(err) = platform::set_always_on_top(self.host_window.canvas.window(), enabled) {
            eprintln!("always-on-top toggle failed: {err}");
            return;
        }
        self.runtime.always_on_top = enabled;
    }

    fn render(&mut self, commands: Vec<DrawCommand>) {
        renderer::render(&mut self.host_window.canvas, &commands);
    }
}

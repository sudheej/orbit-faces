use crate::events::{self, FaceEvent, FaceState};
use crate::lua_host::LuaHost;
use crate::platform;
use crate::renderer::{self, DrawCommand};
use crate::window::{self, HostWindow};
use anyhow::Context;
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

const FRAME_TIME: Duration = Duration::from_millis(16);

pub struct App {
    sdl: sdl3::Sdl,
    host_window: HostWindow,
    lua_host: LuaHost,
    stdin_events: Receiver<FaceEvent>,
    state: FaceState,
    always_on_top: bool,
}

impl App {
    pub fn new(face_dir: PathBuf) -> anyhow::Result<Self> {
        let manifest_path = face_dir.join("manifest.json");
        let manifest = LuaHost::read_manifest(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;

        let sdl = sdl3::init().context("failed to initialize SDL3")?;
        let host_window = window::create(&sdl, manifest.window.width, manifest.window.height)
            .context("failed to create SDL3 host window")?;
        let lua_host = LuaHost::load(face_dir.join(&manifest.script))
            .context("failed to load Lua face script")?;
        let stdin_events = events::spawn_stdin_reader();

        Ok(Self {
            sdl,
            host_window,
            lua_host,
            stdin_events,
            state: FaceState::Idle,
            always_on_top: false,
        })
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let mut event_pump = self
            .sdl
            .event_pump()
            .context("failed to create SDL event pump")?;
        let start = Instant::now();
        let mut last_tick = start;

        self.lua_host.call_load()?;

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
                        keycode: Some(Keycode::T),
                        ..
                    } => self.toggle_always_on_top(),
                    Event::MouseButtonDown { x, y, .. } => self.host_window.begin_drag(x, y),
                    Event::MouseButtonUp { .. } => self.host_window.end_drag(),
                    Event::MouseMotion { x, y, .. } => self.host_window.drag_to(x, y),
                    _ => {}
                }
            }

            while let Ok(face_event) = self.stdin_events.try_recv() {
                match face_event {
                    FaceEvent::StateChanged(next_state) => {
                        self.state = next_state;
                        self.lua_host.call_state_changed(self.state)?;
                    }
                    FaceEvent::AlwaysOnTopChanged(enabled) => {
                        self.set_always_on_top(enabled);
                    }
                }
            }

            self.lua_host.call_update(dt)?;
            let commands = self
                .lua_host
                .draw(self.state, start.elapsed().as_secs_f32())?;
            self.render(commands);

            std::thread::sleep(FRAME_TIME);
        }

        Ok(())
    }

    fn toggle_always_on_top(&mut self) {
        self.set_always_on_top(!self.always_on_top);
    }

    fn set_always_on_top(&mut self, enabled: bool) {
        if let Err(err) = platform::set_always_on_top(self.host_window.canvas.window(), enabled) {
            eprintln!("always-on-top toggle failed: {err}");
            return;
        }
        self.always_on_top = enabled;
    }

    fn render(&mut self, commands: Vec<DrawCommand>) {
        renderer::render(&mut self.host_window.canvas, &commands);
    }
}

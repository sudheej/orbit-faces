use crate::events::StateEvent;
use crate::face_pack::FacePack;
use crate::lua_host::LuaHost;
use crate::renderer::DrawCommand;
use std::time::{Duration, Instant};

pub struct FaceRuntime {
    pub pack: FacePack,
    pub state: String,
    pub debug: bool,
    pub always_on_top: bool,
    lua_host: LuaHost,
    last_commands: Vec<DrawCommand>,
    last_error: Option<String>,
    fps: FpsCounter,
    platform_status: String,
    bridge_mode: String,
    bridge_connected: bool,
    last_received_event: Option<String>,
    last_sent_event: Option<String>,
}

impl FaceRuntime {
    pub fn load(pack: FacePack, platform_status: impl Into<String>) -> anyhow::Result<Self> {
        let lua_host = LuaHost::load(pack.entry_path())?;
        let mut runtime = Self {
            always_on_top: pack.manifest.window.always_on_top,
            pack,
            state: "idle".into(),
            debug: false,
            lua_host,
            last_commands: Vec::new(),
            last_error: None,
            fps: FpsCounter::new(),
            platform_status: platform_status.into(),
            bridge_mode: "stdin".into(),
            bridge_connected: false,
            last_received_event: None,
            last_sent_event: None,
        };
        if let Err(error) = runtime.lua_host.call_load(&runtime.pack.manifest) {
            runtime.record_lua_error("companion.load", error);
        }
        Ok(runtime)
    }

    pub fn change_state(&mut self, mut event: StateEvent) {
        if !self.pack.supports_state(&event.state) {
            eprintln!(
                "warning: face {:?} does not declare state {:?}; falling back to idle",
                self.pack.manifest.name, event.state
            );
            event.state = "idle".into();
        }

        self.state.clone_from(&event.state);
        if let Err(error) = self.lua_host.call_state_changed(&event) {
            self.record_lua_error("companion.state_changed", error);
        }
    }

    pub fn update_and_draw(&mut self, dt: f32, time: f32) -> Vec<DrawCommand> {
        if let Err(error) = self.lua_host.call_update(dt) {
            self.record_lua_error("companion.update", error);
        }

        match self.lua_host.draw(&self.state, time) {
            Ok(commands) => {
                self.last_commands = commands;
                self.last_error = None;
            }
            Err(error) => self.record_lua_error("companion.draw", error),
        }

        self.fps.frame();
        let mut commands = self.last_commands.clone();
        if self.debug {
            self.append_debug_overlay(&mut commands);
        }
        commands
    }

    pub fn hit_test(&mut self, x: f32, y: f32) -> Option<bool> {
        match self.lua_host.hit_test(x, y) {
            Ok(result) => result,
            Err(error) => {
                self.record_lua_error("companion.hit_test", error);
                None
            }
        }
    }

    pub fn enable_bridge_mode(&mut self) {
        self.bridge_mode = "websocket".into();
    }

    pub fn set_bridge_connected(&mut self, connected: bool) {
        self.bridge_connected = connected;
    }

    pub fn record_received_event(&mut self, event_type: impl Into<String>) {
        self.last_received_event = Some(event_type.into());
    }

    pub fn record_sent_event(&mut self, event_type: impl Into<String>) {
        self.last_sent_event = Some(event_type.into());
    }

    fn append_debug_overlay(&self, commands: &mut Vec<DrawCommand>) {
        let bridge_status = if self.bridge_mode == "stdin" {
            "input:stdin".to_owned()
        } else {
            format!(
                "bridge:{} {}",
                self.bridge_mode,
                if self.bridge_connected {
                    "connected"
                } else {
                    "disconnected"
                }
            )
        };
        commands.push(DrawCommand::Rect {
            x: 2.0,
            y: 2.0,
            width: self.pack.manifest.window.width as f32 - 4.0,
            height: if self.last_error.is_some() {
                95.0
            } else {
                85.0
            },
            color: [0, 0, 0, 190],
        });
        let lines = [
            format!("face: {}", self.pack.manifest.name),
            format!("state: {}  fps:{:.0}", self.state, self.fps.value),
            format!("top:{}", if self.always_on_top { "on" } else { "off" }),
            bridge_status,
            format!(
                "recv:{}",
                self.last_received_event.as_deref().unwrap_or("-")
            ),
            format!("sent:{}", self.last_sent_event.as_deref().unwrap_or("-")),
            self.platform_status.clone(),
            self.last_error
                .as_ref()
                .map(|error| format!("lua: {error}"))
                .unwrap_or_default(),
        ];
        for (index, line) in lines.into_iter().enumerate() {
            if !line.is_empty() {
                commands.push(DrawCommand::Text {
                    text: line,
                    x: 6.0,
                    y: 6.0 + index as f32 * 10.0,
                    color: [255, 255, 255, 255],
                });
            }
        }
    }

    fn record_lua_error(&mut self, callback: &str, error: anyhow::Error) {
        let message = format!("{callback}: {error:#}");
        if self.last_error.as_deref() != Some(message.as_str()) {
            eprintln!("Lua face error in {message}");
        }
        self.last_error = Some(message);
    }
}

struct FpsCounter {
    window_started: Instant,
    frames: u32,
    value: f32,
}

impl FpsCounter {
    fn new() -> Self {
        Self {
            window_started: Instant::now(),
            frames: 0,
            value: 0.0,
        }
    }

    fn frame(&mut self) {
        self.frames += 1;
        let elapsed = self.window_started.elapsed();
        if elapsed >= Duration::from_millis(500) {
            self.value = self.frames as f32 / elapsed.as_secs_f32();
            self.frames = 0;
            self.window_started = Instant::now();
        }
    }
}

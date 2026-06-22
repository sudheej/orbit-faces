use crate::events::StateEvent;
use crate::face_pack::FaceManifest;
use crate::renderer::DrawCommand;
use anyhow::Context;
use mlua::{Function, Lua, Table};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

pub struct LuaHost {
    lua: Lua,
}

impl LuaHost {
    pub fn load(script_path: PathBuf) -> anyhow::Result<Self> {
        let lua = Lua::new();
        let script = fs::read_to_string(&script_path)
            .with_context(|| format!("failed to read {}", script_path.display()))?;
        lua.load(&script)
            .set_name(script_path.to_string_lossy().as_ref())
            .exec()?;
        Ok(Self { lua })
    }

    pub fn call_load(&self, manifest: &FaceManifest) -> anyhow::Result<()> {
        if let Some(function) = self.companion_function("load")? {
            let context = self.lua.create_table()?;
            context.set("name", manifest.name.as_str())?;
            context.set("width", manifest.window.width)?;
            context.set("height", manifest.window.height)?;
            function.call::<()>(context)?;
        }
        Ok(())
    }

    pub fn call_state_changed(&self, event: &StateEvent) -> anyhow::Result<()> {
        if let Some(function) = self.companion_function("state_changed")? {
            function.call::<()>(self.create_event_table(event)?)?;
        }
        Ok(())
    }

    pub fn call_update(&self, dt: f32) -> anyhow::Result<()> {
        if let Some(function) = self.companion_function("update")? {
            function.call::<()>(dt)?;
        }
        Ok(())
    }

    pub fn draw(&self, state: &str, time: f32) -> anyhow::Result<Vec<DrawCommand>> {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let ctx = self.create_draw_context(commands.clone(), state, time)?;

        if let Some(function) = self.companion_function("draw")? {
            function.call::<()>(ctx)?;
        }

        let commands = commands.borrow().clone();
        Ok(commands)
    }

    pub fn hit_test(&self, x: f32, y: f32) -> anyhow::Result<Option<bool>> {
        let Some(function) = self.companion_function("hit_test")? else {
            return Ok(None);
        };
        Ok(Some(function.call((x, y))?))
    }

    fn companion_function(&self, name: &str) -> anyhow::Result<Option<Function>> {
        let globals = self.lua.globals();
        let Some(companion) = globals.get::<Option<Table>>("companion")? else {
            return Ok(None);
        };
        Ok(companion.get::<Option<Function>>(name)?)
    }

    fn create_event_table(&self, event: &StateEvent) -> anyhow::Result<Table> {
        let table = self.lua.create_table()?;
        table.set("type", "state")?;
        table.set("state", event.state.as_str())?;
        table.set("emotion", event.emotion.as_deref())?;
        table.set("caption", event.caption.as_deref())?;
        table.set("audio_level", event.audio_level)?;
        Ok(table)
    }

    fn create_draw_context(
        &self,
        commands: Rc<RefCell<Vec<DrawCommand>>>,
        state: &str,
        time: f32,
    ) -> anyhow::Result<Table> {
        let table = self.lua.create_table()?;
        table.set("state", state)?;
        table.set("time", time)?;
        let style = Rc::new(RefCell::new(DrawStyle::default()));

        let clear_commands = commands.clone();
        table.set(
            "clear",
            self.lua
                .create_function(move |_, (r, g, b, a): (u8, u8, u8, u8)| {
                    clear_commands
                        .borrow_mut()
                        .push(DrawCommand::Clear([r, g, b, a]));
                    Ok(())
                })?,
        )?;

        let circle_commands = commands.clone();
        table.set(
            "circle",
            self.lua.create_function(
                move |_, (x, y, radius, r, g, b, a): (f32, f32, f32, u8, u8, u8, u8)| {
                    circle_commands.borrow_mut().push(DrawCommand::Circle {
                        x,
                        y,
                        radius,
                        color: [r, g, b, a],
                    });
                    Ok(())
                },
            )?,
        )?;

        let rect_commands = commands.clone();
        table.set(
            "rect",
            self.lua.create_function(
                move |_, (x, y, width, height, r, g, b, a): (
                    f32,
                    f32,
                    f32,
                    f32,
                    u8,
                    u8,
                    u8,
                    u8,
                )| {
                    rect_commands.borrow_mut().push(DrawCommand::Rect {
                        x,
                        y,
                        width,
                        height,
                        color: [r, g, b, a],
                    });
                    Ok(())
                },
            )?,
        )?;

        let color_style = style.clone();
        table.set(
            "set_color",
            self.lua
                .create_function(move |_, (r, g, b, a): (u8, u8, u8, Option<u8>)| {
                    color_style.borrow_mut().color = [r, g, b, a.unwrap_or(255)];
                    Ok(())
                })?,
        )?;

        let alpha_style = style.clone();
        table.set(
            "set_alpha",
            self.lua.create_function(move |_, alpha: f32| {
                alpha_style.borrow_mut().alpha = alpha.clamp(0.0, 1.0);
                Ok(())
            })?,
        )?;

        let draw_circle_commands = commands.clone();
        let draw_circle_style = style.clone();
        table.set(
            "draw_circle",
            self.lua
                .create_function(move |_, (x, y, radius): (f32, f32, f32)| {
                    draw_circle_commands.borrow_mut().push(DrawCommand::Circle {
                        x,
                        y,
                        radius,
                        color: draw_circle_style.borrow().resolved_color(),
                    });
                    Ok(())
                })?,
        )?;

        let draw_text_commands = commands.clone();
        let draw_text_style = style;
        table.set(
            "draw_text",
            self.lua
                .create_function(move |_, (text, x, y): (String, f32, f32)| {
                    draw_text_commands.borrow_mut().push(DrawCommand::Text {
                        text,
                        x,
                        y,
                        color: draw_text_style.borrow().resolved_color(),
                    });
                    Ok(())
                })?,
        )?;

        table.set("get_time", self.lua.create_function(move |_, ()| Ok(time))?)?;
        let state = state.to_owned();
        table.set(
            "get_state",
            self.lua.create_function(move |_, ()| Ok(state.clone()))?,
        )?;

        Ok(table)
    }
}

#[derive(Debug, Clone)]
struct DrawStyle {
    color: [u8; 4],
    alpha: f32,
}

impl Default for DrawStyle {
    fn default() -> Self {
        Self {
            color: [255, 255, 255, 255],
            alpha: 1.0,
        }
    }
}

impl DrawStyle {
    fn resolved_color(&self) -> [u8; 4] {
        let mut color = self.color;
        color[3] = (color[3] as f32 * self.alpha).round() as u8;
        color
    }
}

#[cfg(test)]
mod tests {
    use super::LuaHost;
    use crate::events::StateEvent;
    use crate::face_pack::FacePack;
    use crate::renderer::DrawCommand;
    use std::path::PathBuf;

    fn example_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/basic_orb")
    }

    #[test]
    fn example_face_handles_state_and_draws() {
        let pack = FacePack::load(example_dir()).unwrap();
        let host = LuaHost::load(pack.entry_path()).unwrap();
        host.call_load(&pack.manifest).unwrap();
        host.call_state_changed(&StateEvent {
            state: "speaking".into(),
            emotion: None,
            caption: None,
            audio_level: Some(0.8),
        })
        .unwrap();
        host.call_update(1.0 / 60.0).unwrap();

        let commands = host.draw("speaking", 0.5).unwrap();

        assert!(matches!(commands.first(), Some(DrawCommand::Clear(_))));
        assert!(commands
            .iter()
            .any(|command| matches!(command, DrawCommand::Circle { .. })));
        assert!(commands
            .iter()
            .any(|command| matches!(command, DrawCommand::Rect { .. })));
    }

    #[test]
    fn all_example_packs_load_and_draw_all_standard_states() {
        let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
        let packs = ["basic_orb", "pixel_pet", "terminal_cube", "minimal_dot"];
        let states = [
            "idle",
            "listening",
            "thinking",
            "speaking",
            "happy",
            "error",
            "sleeping",
        ];

        for pack_name in packs {
            let pack = FacePack::load(examples.join(pack_name)).unwrap();
            let host = LuaHost::load(pack.entry_path()).unwrap();
            host.call_load(&pack.manifest).unwrap();

            for state in states {
                host.call_state_changed(&StateEvent {
                    state: state.into(),
                    emotion: None,
                    caption: Some(format!("{state} test")),
                    audio_level: Some(0.8),
                })
                .unwrap();
                host.call_update(1.0 / 60.0).unwrap();
                let commands = host.draw(state, 0.75).unwrap();
                assert!(
                    !commands.is_empty(),
                    "{pack_name} produced no draw commands for {state}"
                );
            }
        }
    }
}

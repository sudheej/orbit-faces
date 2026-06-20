use crate::events::FaceState;
use crate::renderer::DrawCommand;
use anyhow::Context;
use mlua::{Function, Lua, Table};
use serde::Deserialize;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Debug, Deserialize)]
pub struct FaceManifest {
    pub script: PathBuf,
    pub window: FaceWindow,
}

#[derive(Debug, Deserialize)]
pub struct FaceWindow {
    pub width: u32,
    pub height: u32,
}

pub struct LuaHost {
    lua: Lua,
}

impl LuaHost {
    pub fn read_manifest(path: &Path) -> anyhow::Result<FaceManifest> {
        let text = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn load(script_path: PathBuf) -> anyhow::Result<Self> {
        let lua = Lua::new();
        let script = fs::read_to_string(&script_path)
            .with_context(|| format!("failed to read {}", script_path.display()))?;
        lua.load(&script)
            .set_name(script_path.to_string_lossy().as_ref())
            .exec()?;
        Ok(Self { lua })
    }

    pub fn call_load(&self) -> anyhow::Result<()> {
        if let Some(function) = self.companion_function("load")? {
            function.call::<()>(())?;
        }
        Ok(())
    }

    pub fn call_state_changed(&self, state: FaceState) -> anyhow::Result<()> {
        if let Some(function) = self.companion_function("state_changed")? {
            function.call::<()>(state.as_str())?;
        }
        Ok(())
    }

    pub fn call_update(&self, dt: f32) -> anyhow::Result<()> {
        if let Some(function) = self.companion_function("update")? {
            function.call::<()>(dt)?;
        }
        Ok(())
    }

    pub fn draw(&self, state: FaceState, time: f32) -> anyhow::Result<Vec<DrawCommand>> {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let ctx = self.create_draw_context(commands.clone(), state, time)?;

        if let Some(function) = self.companion_function("draw")? {
            function.call::<()>(ctx)?;
        }

        let commands = commands.borrow().clone();
        Ok(commands)
    }

    fn companion_function(&self, name: &str) -> anyhow::Result<Option<Function>> {
        let globals = self.lua.globals();
        let companion: Table = globals.get("companion")?;
        Ok(companion.get::<Option<Function>>(name)?)
    }

    fn create_draw_context(
        &self,
        commands: Rc<RefCell<Vec<DrawCommand>>>,
        state: FaceState,
        time: f32,
    ) -> anyhow::Result<Table> {
        let table = self.lua.create_table()?;
        table.set("state", state.as_str())?;
        table.set("time", time)?;

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
                    commands.borrow_mut().push(DrawCommand::Rect {
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

        Ok(table)
    }
}

#[cfg(test)]
mod tests {
    use super::LuaHost;
    use crate::events::FaceState;
    use crate::renderer::DrawCommand;
    use std::path::PathBuf;

    fn example_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/basic_orb")
    }

    #[test]
    fn reads_example_manifest() {
        let manifest = LuaHost::read_manifest(&example_dir().join("manifest.json")).unwrap();

        assert_eq!(manifest.script, PathBuf::from("main.lua"));
        assert_eq!(manifest.window.width, 220);
        assert_eq!(manifest.window.height, 220);
    }

    #[test]
    fn example_face_handles_state_and_draws() {
        let host = LuaHost::load(example_dir().join("main.lua")).unwrap();
        host.call_load().unwrap();
        host.call_state_changed(FaceState::Speaking).unwrap();
        host.call_update(1.0 / 60.0).unwrap();

        let commands = host.draw(FaceState::Speaking, 0.5).unwrap();

        assert!(matches!(commands.first(), Some(DrawCommand::Clear(_))));
        assert!(commands
            .iter()
            .any(|command| matches!(command, DrawCommand::Circle { .. })));
        assert!(commands
            .iter()
            .any(|command| matches!(command, DrawCommand::Rect { .. })));
    }
}

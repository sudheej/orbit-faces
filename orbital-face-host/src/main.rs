mod app;
mod events;
mod hitmask;
mod lua_host;
mod platform;
mod renderer;
mod window;

use anyhow::Context;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let face_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("examples/basic_orb"));

    app::App::new(face_dir)
        .context("failed to initialize orbital face host")?
        .run()
}

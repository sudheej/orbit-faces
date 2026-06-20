#[cfg(not(target_os = "linux"))]
mod app;
mod events;
mod hitmask;
mod lua_host;
#[cfg(not(target_os = "linux"))]
mod platform;
mod renderer;
#[cfg(target_os = "linux")]
mod wayland_app;
#[cfg(not(target_os = "linux"))]
mod window;

use anyhow::Context;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    #[cfg(not(target_os = "linux"))]
    platform::set_app_metadata()?;

    let face_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("examples/basic_orb"));

    #[cfg(target_os = "linux")]
    {
        wayland_app::run(face_dir).context("failed to run Wayland orbital face host")
    }

    #[cfg(not(target_os = "linux"))]
    {
        app::App::new(face_dir)
            .context("failed to initialize orbital face host")?
            .run()
    }
}

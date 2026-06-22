#[cfg(not(target_os = "linux"))]
mod app;
mod bridge;
mod events;
mod face_pack;
mod hitmask;
mod lua_host;
#[cfg(not(target_os = "linux"))]
mod platform;
mod renderer;
mod runtime;
#[cfg(target_os = "linux")]
mod wayland_app;
#[cfg(not(target_os = "linux"))]
mod window;

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    #[cfg(not(target_os = "linux"))]
    platform::set_app_metadata()?;

    let options = face_pack::launch_options_from_args()?;

    #[cfg(target_os = "linux")]
    {
        wayland_app::run(options).context("failed to run Wayland orbital face host")
    }

    #[cfg(not(target_os = "linux"))]
    {
        app::App::new(options)
            .context("failed to initialize orbital face host")?
            .run()
    }
}

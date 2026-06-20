use sdl3::video::Window;

#[cfg(windows)]
pub fn set_always_on_top(window: &Window, enabled: bool) -> anyhow::Result<()> {
    let ok = unsafe { sdl3::sys::video::SDL_SetWindowAlwaysOnTop(window.raw(), enabled) };
    anyhow::ensure!(ok, "SDL_SetWindowAlwaysOnTop failed");
    Ok(())
}

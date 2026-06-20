#[cfg(windows)]
mod windows;

use anyhow::Context;
use sdl3::pixels::PixelFormat;
use sdl3::surface::Surface;
use sdl3::video::Window;

#[cfg(not(windows))]
mod windows {
    use anyhow::bail;
    use sdl3::video::Window;

    pub fn set_always_on_top(_window: &Window, _enabled: bool) -> anyhow::Result<()> {
        bail!("always-on-top toggle is implemented only for the Windows prototype")
    }
}

pub use windows::set_always_on_top;

pub fn set_circle_shape(
    window: &Window,
    width: u32,
    height: u32,
    radius: f32,
) -> anyhow::Result<()> {
    let mut shape = Surface::new(width, height, PixelFormat::RGBA32)
        .context("failed to allocate SDL window shape surface")?;
    let pitch = shape.pitch() as usize;
    let center_x = width as f32 * 0.5;
    let center_y = height as f32 * 0.5;
    let radius_squared = radius * radius;

    shape.with_lock_mut(|pixels| {
        pixels.fill(0);

        for y in 0..height {
            for x in 0..width {
                let dx = x as f32 - center_x;
                let dy = y as f32 - center_y;
                if dx * dx + dy * dy <= radius_squared {
                    let offset = y as usize * pitch + x as usize * 4;
                    pixels[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
    });

    let ok = unsafe { sdl3::sys::video::SDL_SetWindowShape(window.raw(), shape.raw()) };
    anyhow::ensure!(ok, "SDL_SetWindowShape failed: {}", sdl3::get_error());
    Ok(())
}

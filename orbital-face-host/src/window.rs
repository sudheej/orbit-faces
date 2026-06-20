use crate::hitmask::CircleHitMask;
use crate::platform;
use anyhow::Context;
use sdl3::render::WindowCanvas;
use sdl3::video::{HitTestResult, WindowFlags, WindowPos};

pub struct HostWindow {
    pub canvas: WindowCanvas,
    hit_mask: CircleHitMask,
    dragging: Option<DragState>,
}

#[derive(Debug, Copy, Clone)]
struct DragState {
    mouse_start_x: f32,
    mouse_start_y: f32,
    window_start_x: i32,
    window_start_y: i32,
}

pub fn create(sdl: &sdl3::Sdl, width: u32, height: u32) -> anyhow::Result<HostWindow> {
    let video = sdl
        .video()
        .context("failed to initialize SDL video subsystem")?;

    let mut builder = video.window("orbital-face-host", width, height);
    builder
        .position_centered()
        .borderless()
        .set_flags(WindowFlags::BORDERLESS | WindowFlags::TRANSPARENT | WindowFlags::NOT_FOCUSABLE);

    let mut window = builder.build().context("failed to build SDL window")?;
    let _ = window.set_opacity(1.0);

    let hit_mask = CircleHitMask {
        width,
        height,
        radius: width.min(height) as f32 * 0.44,
    };
    if let Err(err) = platform::set_circle_shape(&window, width, height, hit_mask.radius) {
        eprintln!("SDL window shape setup failed: {err}");
    }

    let callback_hit_mask = hit_mask;
    if let Err(err) = window.set_hit_test(move |point| {
        if callback_hit_mask.contains(point) {
            HitTestResult::Draggable
        } else {
            HitTestResult::Normal
        }
    }) {
        eprintln!("SDL hit-test setup failed: {err}");
    }

    let canvas = window.into_canvas();

    Ok(HostWindow {
        canvas,
        hit_mask,
        dragging: None,
    })
}

impl HostWindow {
    pub fn begin_drag(&mut self, mouse_x: f32, mouse_y: f32) {
        if !self.hit_mask.contains_xy(mouse_x, mouse_y) {
            return;
        }

        let (window_start_x, window_start_y) = self.canvas.window().position();
        self.dragging = Some(DragState {
            mouse_start_x: mouse_x,
            mouse_start_y: mouse_y,
            window_start_x,
            window_start_y,
        });
    }

    pub fn drag_to(&mut self, mouse_x: f32, mouse_y: f32) {
        let Some(dragging) = self.dragging else {
            return;
        };

        let dx = (mouse_x - dragging.mouse_start_x).round() as i32;
        let dy = (mouse_y - dragging.mouse_start_y).round() as i32;
        self.canvas.window_mut().set_position(
            WindowPos::Positioned(dragging.window_start_x + dx),
            WindowPos::Positioned(dragging.window_start_y + dy),
        );
    }

    pub fn end_drag(&mut self) {
        self.dragging = None;
    }
}

use sdl3::pixels::Color;
use sdl3::render::{BlendMode, FRect, WindowCanvas};

#[derive(Debug, Clone)]
pub enum DrawCommand {
    Clear([u8; 4]),
    Circle {
        x: f32,
        y: f32,
        radius: f32,
        color: [u8; 4],
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: [u8; 4],
    },
}

pub fn render(canvas: &mut WindowCanvas, commands: &[DrawCommand]) {
    canvas.set_blend_mode(BlendMode::Blend);

    for command in commands {
        match *command {
            DrawCommand::Clear(color) => {
                set_color(canvas, color);
                canvas.clear();
            }
            DrawCommand::Circle {
                x,
                y,
                radius,
                color,
            } => draw_filled_circle(canvas, x, y, radius, color),
            DrawCommand::Rect {
                x,
                y,
                width,
                height,
                color,
            } => {
                set_color(canvas, color);
                let _ = canvas.fill_rect(FRect::new(x, y, width.max(0.0), height.max(0.0)));
            }
        }
    }

    canvas.present();
}

fn draw_filled_circle(canvas: &mut WindowCanvas, x: f32, y: f32, radius: f32, color: [u8; 4]) {
    set_color(canvas, color);
    let r = radius.max(0.0).round() as i32;
    let cx = x.round() as i32;
    let cy = y.round() as i32;

    for dy in -r..=r {
        let span = ((r * r - dy * dy) as f32).sqrt().round() as i32;
        let _ = canvas.fill_rect(FRect::new(
            (cx - span) as f32,
            (cy + dy) as f32,
            (span * 2 + 1) as f32,
            1.0,
        ));
    }
}

fn set_color(canvas: &mut WindowCanvas, color: [u8; 4]) {
    canvas.set_draw_color(Color::RGBA(color[0], color[1], color[2], color[3]));
}

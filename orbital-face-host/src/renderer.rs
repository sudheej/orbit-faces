use font8x8::UnicodeFonts;
#[cfg(not(target_os = "linux"))]
use sdl3::pixels::Color;
#[cfg(not(target_os = "linux"))]
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
    Text {
        text: String,
        x: f32,
        y: f32,
        color: [u8; 4],
    },
}

#[cfg(not(target_os = "linux"))]
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
            DrawCommand::Text {
                ref text,
                x,
                y,
                color,
            } => {
                draw_text_canvas(canvas, text, x, y, color);
            }
        }
    }

    canvas.present();
}

#[cfg(not(target_os = "linux"))]
fn draw_filled_circle(canvas: &mut WindowCanvas, x: f32, y: f32, radius: f32, color: [u8; 4]) {
    let radius = radius.max(0.0);
    let min_y = (y - radius - 1.0).floor() as i32;
    let max_y = (y + radius + 1.0).ceil() as i32;

    for pixel_y in min_y..=max_y {
        let sample_y = pixel_y as f32 + 0.5;
        let dy = sample_y - y;
        let solid_half_width = ((radius - 0.5).max(0.0).powi(2) - dy * dy).max(0.0).sqrt();

        if dy.abs() <= (radius - 0.5).max(0.0) {
            let left = (x - solid_half_width).ceil() as i32;
            let right = (x + solid_half_width).floor() as i32;
            if right >= left {
                set_color(canvas, color);
                let _ = canvas.fill_rect(FRect::new(
                    left as f32,
                    pixel_y as f32,
                    (right - left + 1) as f32,
                    1.0,
                ));
            }
        }

        let min_x = (x - radius - 1.0).floor() as i32;
        let max_x = (x + radius + 1.0).ceil() as i32;
        for pixel_x in min_x..=max_x {
            let sample_x = pixel_x as f32 + 0.5;
            let distance = ((sample_x - x).powi(2) + dy * dy).sqrt();
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if coverage <= 0.0 || coverage >= 1.0 {
                continue;
            }

            let mut edge_color = color;
            edge_color[3] = (color[3] as f32 * coverage).round() as u8;
            set_color(canvas, edge_color);
            let _ = canvas.fill_rect(FRect::new(pixel_x as f32, pixel_y as f32, 1.0, 1.0));
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn set_color(canvas: &mut WindowCanvas, color: [u8; 4]) {
    canvas.set_draw_color(Color::RGBA(color[0], color[1], color[2], color[3]));
}

#[cfg(not(target_os = "linux"))]
fn draw_text_canvas(canvas: &mut WindowCanvas, text: &str, x: f32, y: f32, color: [u8; 4]) {
    set_color(canvas, color);
    for (character_index, character) in text.chars().enumerate() {
        let Some(glyph) = font8x8::BASIC_FONTS.get(character) else {
            continue;
        };
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..8 {
                if bits & (1 << column) != 0 {
                    let _ = canvas.fill_rect(FRect::new(
                        x + (character_index * 8 + column) as f32,
                        y + row as f32,
                        1.0,
                        1.0,
                    ));
                }
            }
        }
    }
}

pub fn render_argb8888(pixels: &mut [u8], width: u32, height: u32, commands: &[DrawCommand]) {
    for command in commands {
        match *command {
            DrawCommand::Clear(color) => clear_pixels(pixels, color),
            DrawCommand::Circle {
                x,
                y,
                radius,
                color,
            } => draw_circle_pixels(pixels, width, height, x, y, radius, color),
            DrawCommand::Rect {
                x,
                y,
                width: rect_width,
                height: rect_height,
                color,
            } => draw_rect_pixels(pixels, width, height, x, y, rect_width, rect_height, color),
            DrawCommand::Text {
                ref text,
                x,
                y,
                color,
            } => draw_text_pixels(pixels, width, height, text, x, y, color),
        }
    }
}

fn draw_text_pixels(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    text: &str,
    x: f32,
    y: f32,
    color: [u8; 4],
) {
    let origin_x = x.round() as i32;
    let origin_y = y.round() as i32;
    for (character_index, character) in text.chars().enumerate() {
        let Some(glyph) = font8x8::BASIC_FONTS.get(character) else {
            continue;
        };
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..8 {
                let pixel_x = origin_x + (character_index * 8 + column) as i32;
                let pixel_y = origin_y + row as i32;
                if bits & (1 << column) != 0
                    && pixel_x >= 0
                    && pixel_y >= 0
                    && pixel_x < width as i32
                    && pixel_y < height as i32
                {
                    blend_pixel(pixels, width, pixel_x as u32, pixel_y as u32, color);
                }
            }
        }
    }
}

fn clear_pixels(pixels: &mut [u8], color: [u8; 4]) {
    let pixel = premultiplied_argb(color).to_le_bytes();
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.copy_from_slice(&pixel);
    }
}

fn draw_circle_pixels(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    radius: f32,
    color: [u8; 4],
) {
    let radius = radius.max(0.0);
    let min_x = (x - radius - 1.0).floor().max(0.0) as u32;
    let max_x = (x + radius + 1.0).ceil().min(width as f32) as u32;
    let min_y = (y - radius - 1.0).floor().max(0.0) as u32;
    let max_y = (y + radius + 1.0).ceil().min(height as f32) as u32;

    for pixel_y in min_y..max_y {
        for pixel_x in min_x..max_x {
            let dx = pixel_x as f32 + 0.5 - x;
            let dy = pixel_y as f32 + 0.5 - y;
            let distance = (dx * dx + dy * dy).sqrt();
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            if coverage > 0.0 {
                blend_pixel(
                    pixels,
                    width,
                    pixel_x,
                    pixel_y,
                    with_coverage(color, coverage),
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_rect_pixels(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    rect_width: f32,
    rect_height: f32,
    color: [u8; 4],
) {
    let min_x = x.floor().max(0.0) as u32;
    let max_x = (x + rect_width.max(0.0)).ceil().min(width as f32) as u32;
    let min_y = y.floor().max(0.0) as u32;
    let max_y = (y + rect_height.max(0.0)).ceil().min(height as f32) as u32;

    for pixel_y in min_y..max_y {
        for pixel_x in min_x..max_x {
            blend_pixel(pixels, width, pixel_x, pixel_y, color);
        }
    }
}

fn with_coverage(mut color: [u8; 4], coverage: f32) -> [u8; 4] {
    color[3] = (color[3] as f32 * coverage).round() as u8;
    color
}

fn blend_pixel(pixels: &mut [u8], width: u32, x: u32, y: u32, source: [u8; 4]) {
    let offset = ((y * width + x) * 4) as usize;
    let destination = u32::from_le_bytes(pixels[offset..offset + 4].try_into().unwrap());

    let da = (destination >> 24) & 0xff;
    let dr = (destination >> 16) & 0xff;
    let dg = (destination >> 8) & 0xff;
    let db = destination & 0xff;

    let sa = source[3] as u32;
    let inverse_alpha = 255 - sa;
    let sr = source[0] as u32 * sa / 255;
    let sg = source[1] as u32 * sa / 255;
    let sb = source[2] as u32 * sa / 255;

    let output = ((sa + da * inverse_alpha / 255) << 24)
        | ((sr + dr * inverse_alpha / 255) << 16)
        | ((sg + dg * inverse_alpha / 255) << 8)
        | (sb + db * inverse_alpha / 255);
    pixels[offset..offset + 4].copy_from_slice(&output.to_le_bytes());
}

fn premultiplied_argb(color: [u8; 4]) -> u32 {
    let alpha = color[3] as u32;
    let red = color[0] as u32 * alpha / 255;
    let green = color[1] as u32 * alpha / 255;
    let blue = color[2] as u32 * alpha / 255;
    (alpha << 24) | (red << 16) | (green << 8) | blue
}

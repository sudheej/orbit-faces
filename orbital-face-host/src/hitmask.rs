#[cfg(not(target_os = "linux"))]
use sdl3::rect::Point;

#[derive(Debug, Copy, Clone)]
pub struct CircleHitMask {
    pub width: u32,
    pub height: u32,
    pub radius: f32,
}

impl CircleHitMask {
    pub fn contains_xy(self, x: f32, y: f32) -> bool {
        let cx = self.width as f32 * 0.5;
        let cy = self.height as f32 * 0.5;
        let dx = x - cx;
        let dy = y - cy;
        dx * dx + dy * dy <= self.radius * self.radius
    }

    #[cfg(not(target_os = "linux"))]
    pub fn contains(self, point: Point) -> bool {
        self.contains_xy(point.x as f32, point.y as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::CircleHitMask;

    const MASK: CircleHitMask = CircleHitMask {
        width: 220,
        height: 220,
        radius: 96.8,
    };

    #[test]
    fn includes_center_and_boundary() {
        assert!(MASK.contains_xy(110.0, 110.0));
        assert!(MASK.contains_xy(206.8, 110.0));
    }

    #[test]
    fn excludes_transparent_corners() {
        assert!(!MASK.contains_xy(0.0, 0.0));
        assert!(!MASK.contains_xy(219.0, 219.0));
    }
}

//! Draws tray icons at runtime (modeled on the iPhone battery widget):
//! the outer ring shows the battery level; inside it, a lightning bolt while charging, otherwise a device-kind glyph.
//! All shapes are described analytically, antialiased with 4x4 supersampling.

use crate::bluetooth::DeviceKind;

const SIZE: usize = 32;
const SS: usize = 4;
const C: f32 = SIZE as f32 / 2.0;

// Ring: centerline radius and half-width (in pixels)
const RING_MID: f32 = 14.0;
const RING_HALF: f32 = 1.75;

/// At or below this battery level (and not charging) the ring turns red and a charge reminder pops up
pub const LOW_BATTERY: u8 = 20;

const GREEN: [u8; 3] = [0x34, 0xc7, 0x59];
const RED: [u8; 3] = [0xff, 0x3b, 0x30];

#[derive(Clone, Copy)]
pub struct Style {
    /// Whether the taskbar uses a light theme; decides between a dark or white foreground
    pub light_theme: bool,
}

/// `level` None means the battery level couldn't be read: dashed ring, faded inner glyph ("disconnected" style)
pub fn render(size: usize, level: Option<u8>, charging: bool, kind: DeviceKind, style: Style) -> tray_icon::Icon {
    tray_icon::Icon::from_rgba(pixels(size, level, charging, kind, style), size as u32, size as u32)
        .expect("valid icon size")
}

/// Draw at any edge length; geometry is always in 32px units and scaled proportionally
fn pixels(size: usize, level: Option<u8>, charging: bool, kind: DeviceKind, style: Style) -> Vec<u8> {
    let k = size as f32 / SIZE as f32;
    let (fg, track) = if style.light_theme {
        ([0x1f, 0x1f, 0x1f], ([0x00, 0x00, 0x00], 0.18))
    } else {
        ([0xf2, 0xf2, 0xf2], ([0xff, 0xff, 0xff], 0.28))
    };
    let arc_color = match level {
        Some(l) if l <= LOW_BATTERY && !charging => RED,
        _ => GREEN,
    };
    let frac = level.map_or(0.0, |l| l.min(100) as f32 / 100.0);
    let unknown = level.is_none();
    // No battery level: make the dashed ring a bit more visible, inner glyph semi-transparent
    let track_alpha = if unknown { track.1 * 1.6 } else { track.1 };
    let glyph_alpha = if unknown { 0.45 } else { 1.0 };
    let glyph: fn(f32, f32) -> bool = if charging && level.is_some() {
        bolt
    } else {
        match kind {
            DeviceKind::Keyboard => keyboard,
            DeviceKind::Mouse => mouse,
            DeviceKind::Headphones => headphones,
            DeviceKind::Gamepad => gamepad,
            DeviceKind::Phone => phone,
            DeviceKind::Other => bluetooth_rune,
        }
    };

    let mut px = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (mut track_cov, mut arc_cov, mut glyph_cov) = (0.0, 0.0, 0.0);
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = (x as f32 + (sx as f32 + 0.5) / SS as f32) / k - C;
                    let fy = (y as f32 + (sy as f32 + 0.5) / SS as f32) / k - C;
                    if in_arc(fx, fy, frac) {
                        arc_cov += 1.0;
                    } else if in_ring(fx, fy) && (!unknown || on_dash(fx, fy)) {
                        track_cov += 1.0;
                    } else if glyph(fx, fy) {
                        glyph_cov += 1.0;
                    }
                }
            }
            let n = (SS * SS) as f32;
            let mut out = [0f32; 4]; // premultiplied alpha accumulator
            for (rgb, a) in [(track.0, track_alpha * track_cov / n), (arc_color, arc_cov / n), (fg, glyph_alpha * glyph_cov / n)] {
                for i in 0..3 {
                    out[i] += rgb[i] as f32 * a;
                }
                out[3] += a;
            }
            let i = (y * size + x) * 4;
            if out[3] > 0.0 {
                for c in 0..3 {
                    px[i + c] = (out[c] / out[3]).round() as u8;
                }
                px[i + 3] = (out[3].min(1.0) * 255.0).round() as u8;
            }
        }
    }
    px
}

/// Dashed ring: 12 segments, each 55% solid
fn on_dash(x: f32, y: f32) -> bool {
    let turn = (x.atan2(-y) / std::f32::consts::TAU).rem_euclid(1.0);
    (turn * 12.0).fract() < 0.55
}

fn in_ring(x: f32, y: f32) -> bool {
    ((x * x + y * y).sqrt() - RING_MID).abs() <= RING_HALF
}

/// Battery arc running clockwise from 12 o'clock, with round caps at both ends
fn in_arc(x: f32, y: f32, frac: f32) -> bool {
    if frac <= 0.0 {
        return false;
    }
    if frac >= 1.0 {
        return in_ring(x, y);
    }
    let end = frac * std::f32::consts::TAU;
    let mut theta = x.atan2(-y);
    if theta < 0.0 {
        theta += std::f32::consts::TAU;
    }
    if in_ring(x, y) && theta <= end {
        return true;
    }
    // Round caps
    let cap = |a: f32| {
        let (cx, cy) = (RING_MID * a.sin(), -RING_MID * a.cos());
        (x - cx).powi(2) + (y - cy).powi(2) <= RING_HALF * RING_HALF
    };
    cap(0.0) || cap(end)
}

// ---- Inner glyphs; origin at the center, usable range about +/-9 ----

fn rect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> bool {
    x >= x0 && x <= x1 && y >= y0 && y <= y1
}

fn round_rect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    rect(x, y, x0, y0, x1, y1) && (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

fn circle(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> bool {
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

fn near_segment(x: f32, y: f32, (ax, ay): (f32, f32), (bx, by): (f32, f32), half: f32) -> bool {
    let (dx, dy) = (bx - ax, by - ay);
    let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (x - ax - t * dx).powi(2) + (y - ay - t * dy).powi(2) <= half * half
}

fn in_polygon(x: f32, y: f32, pts: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let ((xi, yi), (xj, yj)) = (pts[i], pts[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn bolt(x: f32, y: f32) -> bool {
    in_polygon(x, y, &[(1.8, -8.5), (-5.0, 1.0), (-0.6, 1.0), (-1.8, 8.5), (5.0, -1.2), (0.6, -1.2)])
}

fn keyboard(x: f32, y: f32) -> bool {
    if !round_rect(x, y, -8.5, -5.0, 8.5, 5.0, 1.5) {
        return false;
    }
    // Cutouts: top row of 4 keys + space bar
    let key = (0..4).any(|i| {
        let x0 = -6.5 + i as f32 * 3.5;
        rect(x, y, x0, -3.0, x0 + 2.0, -1.0)
    });
    let space = rect(x, y, -4.5, 1.2, 4.5, 3.0);
    !(key || space)
}

fn mouse(x: f32, y: f32) -> bool {
    if (x / 5.5).powi(2) + (y / 8.0).powi(2) > 1.0 {
        return false;
    }
    // Cutouts: left/right button divider + bottom edge of the buttons
    let split = rect(x, y, -0.6, -8.0, 0.6, -2.0);
    let edge = rect(x, y, -5.5, -2.6, 5.5, -1.4);
    !(split || edge)
}

fn headphones(x: f32, y: f32) -> bool {
    let r = (x * x + (y - 1.0).powi(2)).sqrt();
    let band = y <= 1.0 && (r - 6.8).abs() <= 1.2;
    let cups = round_rect(x, y, -8.5, 0.0, -4.5, 7.5, 1.3) || round_rect(x, y, 4.5, 0.0, 8.5, 7.5, 1.3);
    band || cups
}

fn gamepad(x: f32, y: f32) -> bool {
    let body = round_rect(x, y, -8.5, -4.5, 8.5, 2.5, 3.0)
        || circle(x, y, -5.2, 2.8, 3.2)
        || circle(x, y, 5.2, 2.8, 3.2);
    if !body {
        return false;
    }
    // Cutouts: D-pad + two buttons
    let dpad = rect(x, y, -7.0, -1.6, -2.6, -0.4) || rect(x, y, -5.4, -3.2, -4.2, 1.2);
    let buttons = circle(x, y, 3.6, -1.8, 1.1) || circle(x, y, 5.8, 0.2, 1.1);
    !(dpad || buttons)
}

fn phone(x: f32, y: f32) -> bool {
    if !round_rect(x, y, -4.8, -8.5, 4.8, 8.5, 2.2) {
        return false;
    }
    // Cut out the screen, leaving a Dynamic Island at its top
    let screen = round_rect(x, y, -2.9, -6.6, 2.9, 6.6, 0.8);
    let island = round_rect(x, y, -1.7, -5.6, 1.7, -4.0, 0.7);
    !screen || island
}

fn bluetooth_rune(x: f32, y: f32) -> bool {
    let pts = [(-4.5, -4.0), (4.0, 3.5), (0.0, 7.5), (0.0, -7.5), (4.0, -3.5), (-4.5, 4.0)];
    pts.windows(2).any(|w| near_segment(x, y, w[0], w[1], 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dump raw RGBA for a few samples for visual inspection: `ICON_DUMP_DIR=... cargo test`
    #[test]
    fn dump_samples() {
        let Ok(dir) = std::env::var("ICON_DUMP_DIR") else { return };
        let samples = [
            ("kb40c", Some(40), true, DeviceKind::Keyboard),
            ("kb75", Some(75), false, DeviceKind::Keyboard),
            ("mouse75", Some(75), false, DeviceKind::Mouse),
            ("hp15", Some(15), false, DeviceKind::Headphones),
            ("pad100", Some(100), false, DeviceKind::Gamepad),
            ("phone46", Some(46), false, DeviceKind::Phone),
            ("other55", Some(55), false, DeviceKind::Other),
            ("none", None, false, DeviceKind::Other),
            ("kb_unknown", None, false, DeviceKind::Keyboard),
            ("mouse_unknown", None, false, DeviceKind::Mouse),
        ];
        for light in [false, true] {
            for (name, level, charging, kind) in samples {
                let px = pixels(SIZE, level, charging, kind, Style { light_theme: light });
                std::fs::write(format!("{dir}/{name}_{}.rgba", if light { "light" } else { "dark" }), px).unwrap();
            }
        }
    }

    /// Generate raw RGBA of the app icon at each size, then scripts/make_ico.py packs them into assets/app.ico / .icns / .png:
    /// `APP_ICON_DIR=... cargo test app_icon`
    #[test]
    fn app_icon() {
        let Ok(dir) = std::env::var("APP_ICON_DIR") else { return };
        for size in [16, 24, 32, 48, 64, 128, 256, 512, 1024] {
            let px = pixels(size, Some(75), false, DeviceKind::Other, Style { light_theme: false });
            std::fs::write(format!("{dir}/{size}.rgba"), px).unwrap();
        }
    }
}

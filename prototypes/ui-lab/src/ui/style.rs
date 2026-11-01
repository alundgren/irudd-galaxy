//! The engine's look: one surface per style role, text that is pure black or pure white,
//! and state colours from a colourblind-safe set, each paired with a shape.

use render::{Color, Panel, PanelShape, Rect, palette};

use super::tree::{Role, StateKind};

/// The colourblind-safe set (Okabe and Ito).
pub mod safe {
    use render::Color;
    pub fn orange() -> Color {
        Color::hex(0xE69F00)
    }
    pub fn sky() -> Color {
        Color::hex(0x56B4E9)
    }
    pub fn green() -> Color {
        Color::hex(0x009E73)
    }
    pub fn yellow() -> Color {
        Color::hex(0xF0E442)
    }
    pub fn blue() -> Color {
        Color::hex(0x0072B2)
    }
    pub fn vermilion() -> Color {
        Color::hex(0xD55E00)
    }
}

/// The shapes paired with state colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Up,
    Down,
    Check,
    Cross,
    Diamond,
    Dot,
    Bar,
    Square,
}

pub fn state_look(kind: StateKind) -> (Color, Shape) {
    match kind {
        StateKind::Gain => (safe::blue(), Shape::Up),
        StateKind::Loss => (safe::vermilion(), Shape::Down),
        StateKind::Ok => (safe::green(), Shape::Check),
        StateKind::Refused => (safe::vermilion(), Shape::Cross),
        StateKind::New => (safe::sky(), Shape::Diamond),
        StateKind::Changed => (safe::orange(), Shape::Dot),
        StateKind::Closed => (safe::vermilion(), Shape::Bar),
        StateKind::Info => (safe::yellow(), Shape::Square),
    }
}

/// Relative luminance of a linear colour seen over the dark sky.
fn luminance(c: Color) -> f32 {
    let [r, g, b, a] = c.to_array();
    (0.2126 * r + 0.7152 * g + 0.0722 * b) * a
}

/// Text on `bg`: pure black or pure white, never grey.
pub fn text_on(bg: Color) -> Color {
    if luminance(bg) > 0.2 {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

/// The ink that outlines things on `bg`.
pub fn ink_on(bg: Color) -> Color {
    if luminance(bg) > 0.2 {
        palette::INK
    } else {
        Color::WHITE
    }
}

/// The night sky behind everything, for text drawn straight on the scene.
pub fn sky() -> Color {
    palette::SPACE_DEEP
}

pub const DARK_PLUM: u32 = 0x2a2036;

/// A role's surface around `rect`. `Plain` has none.
pub fn surface(role: Role, rect: Rect) -> Option<Panel> {
    let p = Panel::new(rect);
    Some(match role {
        Role::Plain => return None,
        Role::Room => Panel {
            fill: palette::CREAM,
            border: palette::INK,
            border_px: 3.0,
            radius: 18.0,
            ..p
        },
        Role::Speech => Panel {
            fill: Color::WHITE,
            border: palette::INK,
            border_px: 3.0,
            radius: 20.0,
            ..p
        },
        Role::Goal => Panel {
            fill: Color::hex(0x173a52),
            border: safe::sky(),
            border_px: 2.0,
            radius: 6.0,
            ..p
        },
        Role::Status => Panel {
            fill: Color::hex(DARK_PLUM),
            border: Color::TRANSPARENT,
            border_px: 0.0,
            radius: 12.0,
            shadow: false,
            ..p
        },
        Role::Action => Panel {
            fill: safe::orange(),
            border: palette::INK,
            border_px: 3.0,
            radius: 12.0,
            shadow: false,
            ..p
        },
        Role::Sign => Panel {
            fill: Color::hex(0xf5d36b),
            border: palette::INK,
            border_px: 2.5,
            radius: 3.0,
            ..p
        },
        Role::Radio => Panel {
            fill: Color::hex(0x10353b),
            border: palette::GLASS,
            border_px: 2.0,
            radius: 14.0,
            ..p
        },
        Role::Receipt => Panel {
            fill: Color::WHITE,
            border: palette::INK,
            border_px: 2.0,
            radius: 8.0,
            shadow: false,
            ..p
        },
    })
}

/// A plain filled shape, no border, no shadow.
pub fn fill(rect: Rect, color: Color, radius: f32) -> Panel {
    Panel {
        fill: color,
        border: Color::TRANSPARENT,
        border_px: 0.0,
        radius,
        shadow: false,
        ..Panel::new(rect)
    }
}

/// A ring: an outline with no fill.
pub fn ring(rect: Rect, color: Color, px: f32, radius: f32, dash: f32) -> Panel {
    Panel {
        fill: Color::TRANSPARENT,
        border: color,
        border_px: px,
        radius,
        shadow: false,
        dash,
        ..Panel::new(rect)
    }
}

/// Draws `shape` centred at `c`, `s` pixels across, inked so it reads on any fill.
pub fn shape(out: &mut Vec<Panel>, shape: Shape, c: render::glam::Vec2, s: f32, color: Color) {
    let sq = |w: f32, h: f32| Rect::new(c.x - w * 0.5, c.y - h * 0.5, w, h);
    let tri = |rot: f32| Panel {
        shape: PanelShape::Triangle,
        rotate: rot,
        border: palette::INK,
        border_px: 1.5,
        radius: 1.0,
        ..fill(sq(s, s * 0.85), color, 0.0)
    };
    let bar = |w: f32, h: f32, rot: f32| Panel {
        rotate: rot,
        border: palette::INK,
        border_px: 1.5,
        ..fill(sq(w, h), color, h * 0.5)
    };
    match shape {
        Shape::Up => out.push(tri(0.0)),
        Shape::Down => out.push(tri(std::f32::consts::PI)),
        Shape::Cross => {
            out.push(bar(s * 1.1, s * 0.3, std::f32::consts::FRAC_PI_4));
            out.push(bar(s * 1.1, s * 0.3, -std::f32::consts::FRAC_PI_4));
        }
        Shape::Check => {
            let short = Rect::new(c.x - s * 0.45, c.y - s * 0.02, s * 0.5, s * 0.28);
            out.push(Panel {
                rotate: std::f32::consts::FRAC_PI_4,
                border: palette::INK,
                border_px: 1.5,
                ..fill(short, color, s * 0.14)
            });
            let long = Rect::new(c.x - s * 0.22, c.y - s * 0.16, s * 0.8, s * 0.28);
            out.push(Panel {
                rotate: -std::f32::consts::FRAC_PI_4,
                border: palette::INK,
                border_px: 1.5,
                ..fill(long, color, s * 0.14)
            });
        }
        Shape::Diamond => out.push(Panel {
            rotate: std::f32::consts::FRAC_PI_4,
            border: palette::INK,
            border_px: 1.5,
            ..fill(sq(s * 0.72, s * 0.72), color, 2.0)
        }),
        Shape::Dot => out.push(Panel {
            border: palette::INK,
            border_px: 1.5,
            ..fill(sq(s * 0.8, s * 0.8), color, s * 0.4)
        }),
        Shape::Bar => out.push(bar(s, s * 0.34, 0.0)),
        Shape::Square => out.push(Panel {
            border: palette::INK,
            border_px: 1.5,
            ..fill(sq(s * 0.7, s * 0.7), color, 1.5)
        }),
    }
}

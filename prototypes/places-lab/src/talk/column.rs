//! A text column at the side: the talk runs as a column down the left of the screen,
//! newest at the bottom and earlier lines moved up above it, each with the speaker's
//! name. In the scene the speaker is marked: their ink turns amber and a ring lies on
//! the floor round their feet.
//!
//! The room's panel holds the right, so the column takes the left. While it shows, the
//! room's picture moves into the space between the two: over, and smaller if it must
//! (the cradle, where Orrin stands far left and the Wren's labels reach the panel), so
//! that nobody's face, sign or socket is under either.

use std::collections::HashMap;

use render::glam::{Vec2, Vec3};
use render::kit::HullParams;
use render::{Camera, Color, MeshHandle, Projection, Rect, Renderer};

use super::{Line, Talk, blend, hung, name_row, next_button, smooth, turned, usable};
use crate::figures::Build;
use crate::places::Places;
use crate::stage::Shown;
use crate::ui::rules;
use crate::widgets::W;
use crate::world::World;

/// The column's width, logical pixels.
const WIDTH: f32 = 260.0;
/// Earlier lines kept above the newest.
const KEEP: usize = 4;
/// Seconds the room takes to move back when the talk ends: short enough that what the
/// pointer rests on settles fast.
const EASE_OUT: f32 = 0.2;
const MARK: u32 = 0xE69F00;

#[derive(Default)]
pub struct Column {
    figures: HashMap<String, (MeshHandle, MeshHandle, Build)>,
    hull: Option<HullParams>,
    /// How far the room is moved over, 0 to 1.
    k: f32,
    place: String,
    /// The room's camera, the column's height, and where the camera moves to for it:
    /// worked out once per line, so the room holds still while people breathe.
    fit: Option<(Camera, f32, Camera)>,
    time: f32,
}

impl Talk for Column {
    fn name(&self) -> &'static str {
        "column"
    }

    fn setup(&mut self, _r: &mut Renderer, world: &World, _model: &Places) {
        self.figures = world
            .figures
            .iter()
            .map(|(id, f)| (id.clone(), (f.body, f.head, f.build)))
            .collect();
        self.hull = Some(world.hull.clone());
    }

    fn widgets(&self, line: &Line) -> Vec<W> {
        let mut col = W::new("column")
            .set("id", "column")
            .set("place", "bottom-left")
            .set("width", WIDTH)
            .set("gap", 8)
            .set("align", "stretch");
        let skip = line.before.len().saturating_sub(KEEP);
        for said in &line.before[skip..] {
            col = col.child(
                W::new("panel")
                    .set("role", "status")
                    .set("pad", 10)
                    .set("gap", 2)
                    .child(W::text(line.name.as_str()).set("style", "small"))
                    .child(W::text(said.as_str()).set("style", "small")),
            );
        }
        vec![
            col.child(
                W::new("panel")
                    .set("role", "speech")
                    .set("pad", 12)
                    .set("gap", 6)
                    .child(name_row(line))
                    .child(W::text(line.text.as_str()))
                    .child(
                        W::new("row")
                            .child(W::new("spacer").set("grow", 1))
                            .child(next_button(line)),
                    ),
            ),
        ]
    }

    fn frame(&mut self, shown: &mut Shown, line: Option<&Line>, viewport: Vec2, dt: f32) {
        self.time += dt;
        if shown.place != self.place {
            self.place = shown.place.clone();
            self.k = 0.0;
        }
        let face = line.and_then(|l| shown.point(&format!("person.{}.face", l.who)));
        if face.is_some() {
            // In at once: the column shows at once, and must never cover the speaker.
            if self.k < 1.0 {
                self.fit = None;
            }
            self.k = 1.0;
        } else {
            self.k = (self.k - dt / EASE_OUT).max(0.0);
        }
        let base = shown.scene.camera;
        if self.k > 0.0
            && let Some(hull) = &self.hull
        {
            // A new line can make the column taller: fit again, at once with it.
            let tall = line.map(height).or(self.fit.map(|f| f.1)).unwrap_or(0.0);
            if self.fit.is_none_or(|(b, t, _)| b != base || t != tall) {
                self.fit = fitted(shown, hull, viewport, tall).map(|c| (base, tall, c));
            }
            if let Some((_, _, fit)) = self.fit {
                shown.scene.camera = blend(&base, &fit, smooth(self.k));
            }
        }
        let Some(line) = line else { return };
        self.mark(shown, line);
    }
}

impl Column {
    /// The speaker in the scene: amber ink, and a ring on the floor at their feet.
    fn mark(&self, shown: &mut Shown, line: &Line) {
        let Some(&(body, head, build)) = self.figures.get(&line.who) else {
            return;
        };
        let amber = Color::hex(MARK);
        for m in &mut shown.scene.meshes {
            if m.mesh == body || m.mesh == head {
                m.material = m.material.with_outline(amber, 6.0);
            }
        }
        let Some(over) = shown.point(&format!("person.{}", line.who)) else {
            return;
        };
        // `person.<who>` is just over the head, its reach a share of the figure's width.
        let scale = over.radius / (build.width * 0.45).max(0.01);
        let feet = over.at - Vec3::Z * (build.top + 0.12) * scale;
        let pulse = 1.0 + 0.06 * (self.time * 3.0).sin();
        let ring = render::Line::circle(
            feet + Vec3::Z * 0.03 * scale,
            build.width * 0.85 * scale * pulse,
            0.07 * scale,
            amber,
        );
        shown.scene.lines.push(ring);
    }
}

/// How tall the column will be for `line`, logical pixels: a generous guess from the
/// text's length, as the layout isn't known here.
fn height(line: &Line) -> f32 {
    let lines = |text: &str, px_per_char: f32, room: f32| {
        (text.chars().count() as f32 * px_per_char / room)
            .ceil()
            .max(1.0)
    };
    let skip = line.before.len().saturating_sub(KEEP);
    let earlier: f32 = line.before[skip..]
        .iter()
        .map(|t| 20.0 + 19.0 + 2.0 + lines(t, 8.0, WIDTH - 20.0) * 19.0 + 8.0)
        .sum();
    let newest = 24.0 + 26.0 + 6.0 + lines(&line.text, 9.2, WIDTH - 24.0) * 23.0 + 6.0 + 42.0;
    earlier + newest
}

/// The room's camera, widened and turned so nothing the room hangs on the scene is under
/// the column (`tall` pixels high at the bottom left) or the room's panel: as large as
/// fits, moved over or up as little as it can.
fn fitted(shown: &Shown, hull: &HullParams, viewport: Vec2, tall: f32) -> Option<Camera> {
    let base = shown.scene.camera;
    let Projection::Perspective { fov_y, near, far } = base.projection else {
        return None;
    };
    let col = Rect::new(
        0.0,
        viewport.y - 16.0 - tall - 14.0,
        16.0 + WIDTH + 14.0,
        tall + 30.0,
    );
    let screen = usable(viewport);
    let first = hung(shown, hull, &base, viewport);
    // Only what the room's own view shows can be asked to stay clear.
    let clear = |cam: &Camera| {
        hung(shown, hull, cam, viewport)
            .iter()
            .zip(&first)
            .all(|(b, f)| {
                !rules::contains(screen, *f)
                    || (rules::contains(screen, *b) && !rules::overlaps(*b, col))
            })
    };
    for i in 0..=10 {
        let s = 1.0 - i as f32 * 0.05;
        let fov = 2.0 * ((fov_y * 0.5).tan() / s).atan();
        let wide = Camera {
            projection: Projection::Perspective {
                fov_y: fov,
                near,
                far,
            },
            ..base
        };
        let under: Vec<Rect> = hung(shown, hull, &wide, viewport)
            .into_iter()
            .zip(&first)
            .filter(|(b, f)| rules::contains(screen, **f) && rules::overlaps(*b, col))
            .map(|(b, _)| b)
            .collect();
        // Over to the right of the column, or up above it: the smaller move first.
        let over = under
            .iter()
            .map(|b| col.x + col.w - b.x)
            .fold(0.0f32, f32::max);
        let up = under
            .iter()
            .map(|b| col.y - b.y - b.h)
            .fold(0.0f32, f32::min);
        let mut moves = [Vec2::new(over, 0.0), Vec2::new(0.0, up)];
        moves.sort_by(|a, b| a.length().total_cmp(&b.length()));
        for m in moves {
            let cam = turned(&wide, viewport, m);
            if clear(&cam) {
                return Some(cam);
            }
        }
    }
    None
}

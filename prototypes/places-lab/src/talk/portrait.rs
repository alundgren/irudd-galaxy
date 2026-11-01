//! A portrait with a name panel: a close-up of the speaker's own figure, head and
//! shoulders on a card at the bottom left, with their name and line beside it. The
//! figure in the room stays where it is: the portrait is the face, the figure is where
//! they are.
//!
//! The renderer has no image textures or second cameras, so the close-up is a small copy
//! of the speaker's meshes set just in front of the room's camera, where it lands on the
//! card's pixels. The screen's bottom edge cuts the body off, like a bust.

use std::collections::HashMap;

use render::art_bible::ArtMaterial;
use render::glam::{Mat4, Vec2, Vec3};
use render::{
    ArtBible, Color, Glow, InkRole, MeshBuilder, MeshHandle, MeshInstance, Rect, Renderer,
};

use super::{Lens, Line, Talk, name_row, next_button};
use crate::figures::{Build, Pose};
use crate::places::Places;
use crate::stage::Shown;
use crate::widgets::W;
use crate::world::World;

/// The card, in logical pixels: this wide, its top this far above the screen's bottom.
const CARD_W: f32 = 200.0;
const CARD_TOP: f32 = 214.0;
const MARGIN: f32 = 16.0;
/// The name panel beside it: the row ends left of the widest room panel.
const TEXT_W: f32 = 440.0;
/// The head (with any hat) is this tall on the card.
const HEAD_PX: f32 = 92.0;
/// How far in front of the eye the copy and its card stand, metres. In front of any
/// set, behind the near plane.
const BUST_DEPTH: f32 = 1.2;
const CARD_DEPTH: f32 = 1.6;

#[derive(Default)]
pub struct Portrait {
    figures: HashMap<String, (MeshHandle, MeshHandle, Build)>,
    card: Option<MeshHandle>,
    time: f32,
    /// The line the card shows, and how long since it began.
    text: String,
    since: f32,
}

/// The card's colour behind each person.
fn card_tint(who: &str) -> Color {
    Color::hex(match who {
        "ilse" => 0x9fd3c7,
        "orrin" => 0xf2c48d,
        "bel" => 0xf6b3a8,
        "juno" => 0xc9e4a3,
        "pim" => 0xc7b8ea,
        _ => 0xf3e6c8,
    })
}

impl Talk for Portrait {
    fn name(&self) -> &'static str {
        "portrait"
    }

    fn setup(&mut self, r: &mut Renderer, world: &World, _model: &Places) {
        self.figures = world
            .figures
            .iter()
            .map(|(id, f)| (id.clone(), (f.body, f.head, f.build)))
            .collect();
        let mut b = MeshBuilder::new();
        b.paint(Color::WHITE, 0.0);
        b.cuboid(Vec3::ONE);
        self.card = Some(r.upload_mesh(&b.build()));
    }

    fn widgets(&self, line: &Line) -> Vec<W> {
        let panel = W::new("panel")
            .set("role", "speech")
            .set("pad", 14)
            .set("gap", 6)
            .set("width", TEXT_W)
            .child(name_row(line))
            .child(W::text(line.text.as_str()))
            .child(
                W::new("row")
                    .child(W::new("spacer").set("grow", 1))
                    .child(next_button(line)),
            );
        // The spacer keeps the card's pixels clear: the bust is drawn in the scene there.
        vec![
            W::new("row")
                .set("id", "portrait")
                .set("place", "bottom-left")
                .set("width", CARD_W + 12.0 + TEXT_W)
                .set("gap", 12)
                .set("align", "end")
                .child(W::new("spacer").set("size", CARD_W))
                .child(panel),
        ]
    }

    fn frame(&mut self, shown: &mut Shown, line: Option<&Line>, viewport: Vec2, dt: f32) {
        self.time += dt;
        let Some(line) = line else { return };
        if line.text != self.text {
            self.text = line.text.clone();
            self.since = 0.0;
        }
        self.since += dt;
        let (Some(&(body, head, build)), Some(card)) = (self.figures.get(&line.who), self.card)
        else {
            return;
        };
        let lens = Lens::new(&shown.scene.camera, viewport);
        let top = viewport.y - CARD_TOP;
        // Past the bottom edge, so the body runs off the screen.
        let rect = Rect::new(MARGIN, top, CARD_W, CARD_TOP + 40.0);
        let scene = &mut shown.scene;
        let bible = ArtBible::bundled();

        // The card behind the bust.
        let k = lens.per_px(CARD_DEPTH);
        let at = lens.at(rect.center(), CARD_DEPTH);
        let m = Mat4::from_cols(
            (lens.right * rect.w * k).extend(0.0),
            (lens.fwd * 0.02).extend(0.0),
            (lens.up * rect.h * k).extend(0.0),
            at.extend(1.0),
        );
        scene.meshes.push(
            MeshInstance::new(card, m).with_material(
                bible
                    .material(InkRole::Prop)
                    .with_tint(card_tint(&line.who))
                    .with_outline(render::palette::INK, 4.0),
            ),
        );

        // The bust: the speaker's own meshes, scaled so the head fills the card's top,
        // facing the camera. The head nods while the line is new.
        let k = lens.per_px(BUST_DEPTH);
        // The head fills the card's top; broad people are kept inside the card.
        let s = (HEAD_PX * k / (build.top - build.neck).max(0.1))
            .min(CARD_W * 0.8 * k / build.width.max(0.1));
        let crown = lens.at(Vec2::new(rect.x + CARD_W * 0.5, top + 18.0), BUST_DEPTH);
        let feet = crown - lens.up * build.top * s;
        let basis = Mat4::from_cols(
            lens.right.extend(0.0),
            lens.fwd.extend(0.0),
            lens.up.extend(0.0),
            feet.extend(1.0),
        ) * Mat4::from_scale(Vec3::splat(s));
        let saying = (line.text.len() as f32 * 0.05).clamp(1.0, 4.0);
        let talk = if self.since < saying {
            1.0 - self.since / saying
        } else {
            0.0
        };
        let t = self.time;
        let pose = Pose {
            breathe: 1.0 + 0.012 * (t * 1.9).sin(),
            head_turn: 0.12 * (t * 0.6).sin(),
            head_nod: 0.05 * (t * 1.3).sin() + talk * 0.07 * (t * 9.0).sin(),
            ..Pose::still()
        };
        let (b, h) = pose.transforms(Vec3::ZERO, 0.0, &build);
        let mat = bible.material(InkRole::Prop);
        scene
            .meshes
            .push(MeshInstance::new(body, basis * b).with_material(mat));
        scene
            .meshes
            .push(MeshInstance::new(head, basis * h).with_material(mat));
        // A soft light behind the head.
        let face_px = Vec2::new(rect.x + CARD_W * 0.5, top + 18.0 + HEAD_PX * 0.55);
        scene.glows.push(Glow {
            position: lens.at(face_px, (BUST_DEPTH + CARD_DEPTH) * 0.5),
            radius: 70.0 * lens.per_px((BUST_DEPTH + CARD_DEPTH) * 0.5),
            color: Color::hex(0xfff1c8).with_alpha(0.45),
        });
    }
}

//! The hullwright's cradle in the world (#47, 3.2): with the wharf's room open, the
//! camera comes right in on the wharf's module and the flagship is drawn there, lifted
//! into the cradle, large and nose up, with a ring on each socket this wharf works on.
//! A crane stands beside it; when a part is fitted it swings over, lowers the part,
//! three bolts flash and the hull dips and settles ([`crate::ui::rooms::fit::Fitting`],
//! view only).
//!
//! Everything but the meshes is pure geometry over the world: where the cradle sits,
//! where the camera goes, and each socket on screen ([`screen_sockets`]), so the room's
//! drop targets are hit-tested with no renderer.

use galaxy_contract::parts::StationPart;
use galaxy_contract::prelude::*;
use galaxy_core::History;
use render::art_bible::{ArtBible, ArtMaterial, InkRole};
use render::glam::{Mat4, Vec2 as G2, Vec3};
use render::kit::{self, CradlePose, LookId, ShipFitLook, Socket, SocketRing};
use render::{Camera, Color, Glow, Line, MeshInstance, Rect, Renderer, Scene, palette};

use super::{TILT, View, fit_look, recipes};
use crate::ui::rooms::fit::{self, Fitting, Over, Source, WharfUi};

/// The ship's length in the cradle, in world units.
pub const SPAN: f32 = 44.0;
/// The share of the room left of the panel (its height) the ship's length fills.
const FILL: f32 = 0.5;
/// Where across the free room the ship sits: right of the middle, so the socket labels
/// fit on its left.
const ACROSS: f32 = 0.62;

/// Where the cradle holds the ship: on the wharf's module.
pub fn center(world: &WorldState, part: &StationPart) -> Option<G2> {
    super::modules::module_point(world, part).map(|(p, _)| p)
}

/// The kit's socket for one of ours: the rack sits in the utility bay but has no ring
/// of its own (its modules show beside the ship).
pub fn kit_socket(world: &WorldState, s: &SocketId) -> Option<Socket> {
    let def = world.progress.rules.fit.sockets.get(s)?;
    if def.rack {
        return None;
    }
    Some(match def.place.as_str() {
        "nose" => Socket::Nose,
        "utility" => Socket::Utility,
        "belly" => Socket::Belly,
        "tail" => Socket::Tail,
        _ => return None,
    })
}

fn flagship_hull(world: &WorldState) -> Option<HullId> {
    galaxy_contract::fit::flagship(world).map(|s| s.hull.clone())
}

/// The flagship's sockets the wharf at `station` works on, with their rings, in the
/// hull's order. Cinder Hook's rough rigger shows only her drill mount.
pub fn shown_sockets(world: &WorldState, station: &StationId) -> Vec<(SocketId, Socket)> {
    let Some(hull) = flagship_hull(world) else {
        return Vec::new();
    };
    let Some(wharf) = world.progress.rules.fit.wharves.get(station) else {
        return Vec::new();
    };
    galaxy_contract::fit::hull_fit(world, &hull)
        .map(|h| h.sockets.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|s| wharf.sockets.contains(s))
        .filter_map(|s| kit_socket(world, &s).map(|k| (s, k)))
        .collect()
}

/// How high the hull rides during the fit: it dips as the bolts go home, then settles.
fn dip(t: f32) -> f32 {
    let u = ((t - 1.15) / 0.35).clamp(0.0, 1.0);
    1.0 - 0.45 * (std::f32::consts::PI * u).sin() * (1.0 - 0.4 * u)
}

/// The ship's pose in the cradle at `center`, dipping during a fit.
pub fn pose(world: &WorldState, center: G2, fitting: Option<&Fitting>) -> Option<CradlePose> {
    let params = recipes::hull_params(&flagship_hull(world)?);
    let mut pose = CradlePose::fit(center, SPAN, &params);
    if let Some(f) = fitting {
        pose.lift *= dip(f.t);
    }
    Some(pose)
}

/// Where the camera goes with the wharf's room open: the cradle in the middle of the
/// room the panel leaves on its left (`free`, on the canvas), the ship filling most of
/// its height. `(center, zoom)` in world units.
pub fn camera_goal(
    world: &WorldState,
    part: &StationPart,
    free: Rect,
    canvas: G2,
) -> Option<(G2, f32)> {
    let c = center(world, part)?;
    // World units per canvas pixel, so the ship's length fills its share.
    let wpp = SPAN / (free.h * FILL).max(80.0);
    let zoom = wpp * canvas.y;
    let at = G2::new(free.x + free.w * ACROSS, free.y + free.h * 0.5);
    // The camera's centre is the canvas's; shift it so the cradle lands on `at`
    // (screen y runs down, world y up).
    let shift = G2::new(canvas.x * 0.5 - at.x, at.y - canvas.y * 0.5) * wpp;
    Some((c + shift, zoom))
}

/// A socket in the cradle, on the canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenSocket {
    pub socket: SocketId,
    pub kit: Socket,
    /// The ring's centre and radius, canvas pixels.
    pub at: G2,
    pub radius: f32,
}

/// The smallest a socket's target is, canvas pixels: small rings stay easy to hit.
pub const MIN_TARGET: f32 = 24.0;

/// The shown sockets on the canvas, through `camera`.
pub fn screen_sockets(
    world: &WorldState,
    station: &StationId,
    part: &StationPart,
    camera: &Camera,
    viewport: G2,
    scale: f32,
) -> Vec<ScreenSocket> {
    let (Some(c), Some(hull)) = (center(world, part), flagship_hull(world)) else {
        return Vec::new();
    };
    let Some(pose) = pose(world, c, None) else {
        return Vec::new();
    };
    let params = recipes::hull_params(&hull);
    let on_screen = kit::cradle_sockets(&params, &pose, camera, viewport);
    shown_sockets(world, station)
        .into_iter()
        .filter_map(|(socket, k)| {
            let s = on_screen.iter().find(|s| s.socket == k)?;
            Some(ScreenSocket {
                socket,
                kit: k,
                at: s.screen? / scale,
                radius: (s.radius_px / scale).max(MIN_TARGET),
            })
        })
        .collect()
}

/// About where the ship in the cradle covers the canvas, for keeping signs off it.
pub fn canvas_rect(
    world: &WorldState,
    part: &StationPart,
    camera: &Camera,
    viewport: G2,
    scale: f32,
) -> Option<Rect> {
    let c = center(world, part)?;
    let at = camera.world_to_screen(c.extend(0.0), viewport)? / scale;
    let px = SPAN / (camera.world_per_pixel(viewport) / scale).max(f32::EPSILON);
    Some(Rect::new(
        at.x - px * 0.45,
        at.y - px * 0.62,
        px * 0.9,
        px * 1.24,
    ))
}

/// The camera for the docked view at `center` and `zoom`: the play camera.
pub fn camera_at(center: G2, zoom: f32) -> Camera {
    Camera::top_down(center, zoom).with_tilt(TILT)
}

/// What the cradle shows this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct CradleSpec {
    pub hull: HullId,
    pub pose: CradlePose,
    /// What the hull wears (without a part the crane is still bringing).
    pub look: ShipFitLook,
    /// The held part, drawn as a ghost on the socket it's over.
    pub ghost: Option<LookId>,
    pub rings: [SocketRing; 4],
    /// The part on the crane, its socket and the animation's seconds.
    pub carried: Option<(LookId, Socket, f32)>,
    /// The crane's arm while it isn't fitting: parked.
    pub crane: G2,
}

/// The cradle for the wharf room `part` at `station`, from the room's state `ui`.
pub fn spec(
    world: &WorldState,
    history: &History,
    station: &StationId,
    part: &StationPart,
    ui: &WharfUi,
) -> Option<CradleSpec> {
    let c = center(world, part)?;
    let hull = flagship_hull(world)?;
    let pose = pose(world, c, ui.fitting.as_ref())?;
    let shown = shown_sockets(world, station);
    let mut look = fit_look::look_for(world, &world.player.fittings);
    let carried = ui.fitting.as_ref().and_then(|f| {
        let k = kit_socket(world, &f.socket)?;
        look.set(k, None);
        let (_, l) = fit_look::look_of(world, &f.part)?;
        Some((l, k, f.t))
    });
    let holding = ui
        .held
        .as_ref()
        .filter(|h| h.from != Source::Socket)
        .map(|h| &h.part);
    let views = galaxy_progress::hullwright::sockets(world, history, station, holding);
    let rings = Socket::ALL.map(|k| {
        let Some((s, _)) = shown.iter().find(|(_, x)| *x == k) else {
            return SocketRing::None;
        };
        if ui.fitting.as_ref().is_some_and(|f| &f.socket == s) {
            SocketRing::Empty
        } else if views.iter().any(|v| &v.socket == s && v.takes) {
            SocketRing::DropTarget
        } else if fit::occupant(world, s).is_some() {
            SocketRing::Fitted
        } else {
            SocketRing::Empty
        }
    });
    let ghost = match (&ui.held, &ui.over) {
        (Some(h), Some(Over::Socket(s))) if h.from != Source::Socket => {
            fit::socket_of(world, &h.part)
                .filter(|p| p == s)
                .and_then(|_| fit_look::look_of(world, &h.part))
                .map(|(_, l)| l)
        }
        _ => None,
    };
    Some(CradleSpec {
        hull,
        pose,
        look,
        ghost,
        rings,
        carried,
        crane: crane_rest(c),
    })
}

/// The crane's pivot, beside the cradle on the right, past the wing tip.
fn crane_pivot(c: G2) -> G2 {
    c + G2::new(SPAN * 0.66, SPAN * 0.12)
}

/// Where the crane's hook rests: parked, its arm pointing up and out off the ship.
fn crane_rest(c: G2) -> G2 {
    crane_pivot(c) + G2::new(SPAN * 0.04, SPAN * 0.4)
}

fn smooth(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// The hook between `from` and `to`, swung about the pivot (angle and reach eased
/// together), `k` of the way.
fn swing(pivot: G2, from: G2, to: G2, k: f32) -> G2 {
    let (a, b) = (from - pivot, to - pivot);
    let (ra, rb) = (a.length(), b.length());
    let (ta, mut tb) = (a.y.atan2(a.x), b.y.atan2(b.x));
    while tb - ta > std::f32::consts::PI {
        tb -= std::f32::consts::TAU;
    }
    while ta - tb > std::f32::consts::PI {
        tb += std::f32::consts::TAU;
    }
    let k = smooth(k);
    let t = ta + (tb - ta) * k;
    let r = ra + (rb - ra) * k;
    pivot + G2::new(t.cos(), t.sin()) * r
}

/// The animation's phases, in seconds: the crane swings over, lowers the part, the
/// bolts go home, the hull settles and the crane swings back.
const SWING: f32 = 0.45;
const LOWER: f32 = 0.85;
const BOLTS: [f32; 3] = [0.9, 1.03, 1.16];
const FLASH: f32 = 0.12;
const BACK: f32 = 1.2;

impl View {
    /// Uploads what the cradle draws this frame (once each; nothing per frame).
    pub fn prepare_cradle(&mut self, r: &mut Renderer, spec: &CradleSpec) {
        self.hull_mesh(r, &spec.hull, None, spec.look);
        if let Some(g) = spec.ghost {
            self.part_mesh(r, &spec.hull, g);
        }
        if let Some((l, ..)) = spec.carried {
            let hull = spec.hull.clone();
            self.solids
                .entry((hull.clone(), l))
                .or_insert_with(|| r.upload_mesh(&kit::part(l, &recipes::hull_params(&hull))));
        }
    }

    /// Draws the cradle (the ship, its shadow, the rings, a ghost part) and the crane
    /// into `scene`. Needs [`View::prepare_cradle`] first; what isn't uploaded is left
    /// out.
    pub fn draw_cradle(&self, scene: &mut Scene, spec: &CradleSpec, viewport: G2) {
        let Some(&ship) = self.hulls.get(&(spec.hull.clone(), None, spec.look)) else {
            return;
        };
        let params = recipes::hull_params(&spec.hull);
        let cradle = kit::Cradle {
            ship,
            hull: params.clone(),
            look: spec.look,
            pose: spec.pose,
            rings: spec.rings,
            ghost: spec
                .ghost
                .and_then(|g| self.parts.get(&(spec.hull.clone(), g)).copied()),
            shadow: kit::CRADLE_SHADOW,
        };
        let sockets = cradle.draw(scene, viewport);
        let c = spec.pose.center;
        let pivot = crane_pivot(c);
        let top = spec.pose.lift + SPAN * 0.8;
        let mut hook = spec.crane;
        if let Some((look, socket, t)) = spec.carried {
            let at = spec
                .pose
                .transform()
                .transform_point3(params.socket(socket));
            let over = at.truncate();
            hook = if t < SWING {
                swing(pivot, spec.crane, over, t / SWING)
            } else if t < BACK {
                over
            } else {
                swing(
                    pivot,
                    over,
                    spec.crane,
                    (t - BACK) / (fit::FIT_SECONDS - BACK),
                )
            };
            // The part hangs under the hook, high, then comes down onto its socket.
            let high = top - spec.pose.lift - SPAN * 0.1;
            let z = if t < SWING {
                high
            } else {
                high * (1.0 - smooth((t - SWING) / (LOWER - SWING)))
            };
            let carry = if t < SWING { hook - over } else { G2::ZERO };
            if let Some(&mesh) = self.solids.get(&(spec.hull.clone(), look)) {
                let m = Mat4::from_translation(carry.extend(z)) * spec.pose.transform();
                scene.meshes.push(
                    MeshInstance::new(mesh, m)
                        .with_material(ArtBible::bundled().material(InkRole::Ship)),
                );
            }
            // Three bolts, one after another, round the socket.
            let ring = sockets.iter().find(|s| s.socket == socket);
            let r = ring.map_or(SPAN * 0.08, |s| s.radius * 0.75);
            let base = ring.map_or(at, |s| s.world);
            for (i, start) in BOLTS.iter().enumerate() {
                if (*start..start + FLASH).contains(&t) {
                    let a = std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / 3.0;
                    let p = base + Vec3::new(a.cos(), a.sin(), 0.0) * r;
                    scene.glows.push(Glow {
                        position: p,
                        radius: SPAN * 0.09,
                        color: Color::WHITE,
                    });
                    scene.glows.push(Glow {
                        position: p,
                        radius: SPAN * 0.16,
                        color: palette::LANTERN.with_alpha(0.8),
                    });
                }
            }
        }
        crane(scene, pivot, hook, top);
    }
}

/// The crane: a brass arm inked dark from a teal pivot to its hook.
fn crane(scene: &mut Scene, pivot: G2, hook: G2, z: f32) {
    let (p, h) = (pivot.extend(z), hook.extend(z));
    scene
        .lines
        .push(Line::path(vec![p, h], SPAN * 0.075, palette::INK));
    scene
        .lines
        .push(Line::path(vec![p, h], SPAN * 0.042, palette::MUSTARD));
    scene
        .lines
        .push(Line::circle(p, SPAN * 0.05, SPAN * 0.05, palette::INK));
    scene
        .lines
        .push(Line::circle(p, SPAN * 0.05, SPAN * 0.03, palette::TEAL));
    scene
        .lines
        .push(Line::circle(h, SPAN * 0.035, SPAN * 0.02, palette::INK));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hull_dips_and_settles() {
        assert_eq!(dip(0.0), 1.0);
        assert!(dip(1.3) < 0.8);
        assert!((dip(fit::FIT_SECONDS) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn the_crane_swings_from_rest_to_the_socket() {
        let c = G2::ZERO;
        let (pivot, rest) = (crane_pivot(c), crane_rest(c));
        let over = G2::new(0.0, SPAN * 0.4);
        assert!(swing(pivot, rest, over, 0.0).distance(rest) < 1e-3);
        assert!(swing(pivot, rest, over, 1.0).distance(over) < 1e-3);
        let mid = swing(pivot, rest, over, 0.5);
        let (ra, rb) = ((rest - pivot).length(), (over - pivot).length());
        let r = (mid - pivot).length();
        assert!(
            r >= ra.min(rb) - 1e-3 && r <= ra.max(rb) + 1e-3,
            "it swings"
        );
    }
}

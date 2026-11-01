//! Filming: a session drawn frame by frame on a fixed presentation clock, as the window
//! would show it, into a film folder (`galaxy_motion::film` has the format).
//!
//! Frame `k` is shown at `k / fps` seconds. By then the sim has run `floor(k * 30 / fps)`
//! ticks, and the frame is drawn that far between the last two of them (`alpha`), with
//! the same presentation time the window would pass, so interpolation, the camera's
//! follow and every timed effect (the course settling, a ring closing) are filmed exactly
//! as `galaxy play` draws them at that frame rate. Scenes are built for every frame from
//! a warm-up before the first filmed one, so effects that depend on what came before are
//! already under way; only the filmed frames are drawn and probed.
//!
//! Without a renderer the film has probes only: where things are on screen, and every
//! line and dot, with no GPU.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, sync_channel};
use std::sync::{Arc, Mutex};

use galaxy_api::engine::{BODY, CAMERA, HOT_LOADED, MODEL, POSITION, TICKS_PER_SECOND};
use galaxy_module::module::Module;
use galaxy_motion::film::{Cam, Change, FORMAT, Frame, LineProbe, Manifest, Outline, Thing};
use galaxy_render::glam::Vec2;
use galaxy_render::{Camera, Line, LineStyle, Renderer, RgbaImage, Scene};
use galaxy_sim::changes;
use galaxy_sim::recording::RecEvent;
use galaxy_sim::scenario::InputsFile;
use galaxy_sim::shape::Shapes;
use galaxy_sim::{BuildStamp, Input, Recording, Replay, Session, World};
use serde_json::Value;

use crate::{View, camera_holder};

/// Seconds of scenes built, but not filmed, before the first filmed frame.
pub const WARM_UP_SECONDS: u64 = 10;

/// Frames a second, when not told.
pub const DEFAULT_FPS: u32 = 60;

/// What a film steps: a scripted session or a recording being replayed.
pub trait Stepper {
    fn world(&self) -> &World;
    /// Runs the next tick. True when modules were hot-loaded before it, so the view
    /// loads their models again.
    fn step(&mut self) -> Result<bool, String>;
    fn modules(&self) -> Vec<&Module>;
    /// The inputs the next step to `tick` applies.
    fn inputs(&self, tick: u64) -> &[Input];
}

/// A session run from an inputs file: its inputs and hot-loads on their ticks.
pub struct Scripted {
    pub session: Session,
    inputs: BTreeMap<u64, Vec<Input>>,
    loads: BTreeMap<u64, Vec<Module>>,
}

impl Scripted {
    /// `session` with `script`'s inputs, each checked against it, and its hot-loads read.
    pub fn new(session: Session, script: &InputsFile) -> Result<Scripted, String> {
        for input in &script.inputs {
            session
                .check_input(input)
                .map_err(|e| format!("input `{}` on tick {}: {e}", input.name, input.tick))?;
        }
        let mut loads: BTreeMap<u64, Vec<Module>> = BTreeMap::new();
        for (tick, dir) in &script.hot_loads {
            loads
                .entry(*tick)
                .or_default()
                .push(Module::read(dir).map_err(|f| f.message)?);
        }
        Ok(Scripted {
            session,
            inputs: script.by_tick(),
            loads,
        })
    }
}

impl Stepper for Scripted {
    fn world(&self) -> &World {
        &self.session.world
    }

    fn step(&mut self) -> Result<bool, String> {
        let tick = self.session.world.tick + 1;
        let loads = self.loads.remove(&tick).unwrap_or_default();
        let hot = !loads.is_empty();
        for module in loads {
            let id = module.id().to_owned();
            self.session
                .hot_load(module)
                .map_err(|e| format!("the hot-load of `{id}` before tick {tick}: {e}"))?;
        }
        self.session
            .step(self.inputs.get(&tick).map_or(&[][..], Vec::as_slice))?;
        Ok(hot)
    }

    fn modules(&self) -> Vec<&Module> {
        self.session.modules().collect()
    }

    fn inputs(&self, tick: u64) -> &[Input] {
        self.inputs.get(&tick).map_or(&[][..], Vec::as_slice)
    }
}

impl Stepper for Replay<'_> {
    fn world(&self) -> &World {
        &self.session.world
    }

    fn step(&mut self) -> Result<bool, String> {
        let facts = self.session.world.facts.len();
        if !Replay::step(self)? {
            return Err(format!(
                "the recording ends at tick {}, before the film does",
                self.end()
            ));
        }
        Ok(self.session.world.facts[facts..]
            .iter()
            .any(|f| f.kind == HOT_LOADED))
    }

    fn modules(&self) -> Vec<&Module> {
        self.session.modules().collect()
    }

    fn inputs(&self, tick: u64) -> &[Input] {
        self.inputs_at(tick)
    }
}

/// Runs `stepper`'s next tick, keeping what changed in `changes` when the tick is one
/// the film shows. True when modules were hot-loaded.
fn step_noting(
    stepper: &mut dyn Stepper,
    filmed: std::ops::RangeInclusive<u64>,
    out: &mut Vec<Change>,
) -> Result<bool, String> {
    let tick = stepper.world().tick + 1;
    if !filmed.contains(&tick) {
        return stepper.step();
    }
    let t = tick as f32 / TICKS_PER_SECOND as f32;
    let at = |kind: &str, what: String| Change {
        tick,
        t,
        kind: kind.into(),
        what,
    };
    for input in stepper.inputs(tick) {
        out.push(at("input", changes::input(input)));
    }
    let before = changes::orders(stepper.world());
    let facts = stepper.world().facts.len();
    let hot = stepper.step()?;
    for (kind, what) in changes::since(&before, stepper.world(), facts) {
        out.push(at(kind, what));
    }
    Ok(hot)
}

/// The zooms a recording kept, for filming it as the player saw it.
pub fn recorded_zooms(rec: &Recording) -> Vec<(u64, Zoom)> {
    rec.events
        .iter()
        .filter_map(|e| match e {
            RecEvent::Zoom { tick, view } => Some((*tick, Zoom::View(*view))),
            _ => None,
        })
        .collect()
}

/// A change of zoom, once its tick has run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    /// Wheel steps, as from an inputs file (positive is closer).
    Steps(f32),
    /// A view height, as a recording keeps it.
    View(f32),
}

/// How to film.
pub struct Shoot {
    pub fps: u32,
    /// The ticks to film from and to (presentation from `from / 30` to `to / 30`
    /// seconds).
    pub from: u64,
    pub to: u64,
    /// The frame in physical pixels, and the window scale it stands for.
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub out: PathBuf,
    /// Zoom changes by tick.
    pub zooms: Vec<(u64, Zoom)>,
    /// What is being filmed, kept in the manifest.
    pub source: Value,
}

/// When frame `k` is shown at `fps`: the ticks run by then, how far it is drawn between
/// the last two, and the presentation time.
pub fn schedule(k: u64, fps: u32) -> (u64, f32, f32) {
    let fps = u64::from(fps.max(1));
    let scaled = k * u64::from(TICKS_PER_SECOND);
    (
        scaled / fps,
        (scaled % fps) as f32 / fps as f32,
        (k as f64 / fps as f64) as f32,
    )
}

/// Films `stepper` from where it stands into `shoot.out`, drawing with `renderer` if
/// there is one. Returns the manifest it wrote.
pub fn shoot(
    stepper: &mut dyn Stepper,
    view: &mut View,
    mut renderer: Option<&mut Renderer>,
    shoot: &Shoot,
) -> Result<Manifest, String> {
    let fps = shoot.fps.clamp(1, 240);
    let tps = u64::from(TICKS_PER_SECOND);
    let first = (shoot.from * u64::from(fps)).div_ceil(tps);
    let last = shoot.to * u64::from(fps) / tps;
    if last < first || shoot.to <= shoot.from {
        return Err(format!(
            "nothing to film from tick {} to tick {}: the end must come after the start",
            shoot.from, shoot.to
        ));
    }
    if last - first > 60 * 60 * u64::from(fps) {
        return Err("a film is at most an hour long".into());
    }
    let start = stepper.world().tick;
    if shoot.from < start {
        return Err(format!(
            "the session starts at tick {start}, after tick {} where the film would start",
            shoot.from
        ));
    }
    let warm = first.saturating_sub(WARM_UP_SECONDS * u64::from(fps));
    let draw = renderer.is_some();
    prepare(&shoot.out, draw)?;
    if let Some(r) = renderer.as_deref_mut() {
        r.resize(shoot.width, shoot.height);
        r.set_scale_factor(shoot.scale);
    }
    let size = (shoot.width, shoot.height);
    let logical = Vec2::new(shoot.width as f32, shoot.height as f32) / shoot.scale;
    let mut zooms = shoot.zooms.clone();
    zooms.sort_by_key(|z| z.0);
    let mut zooms = zooms.into_iter().peekable();
    let mut frames = Vec::new();
    let mut changes = Vec::new();
    let mut hash: Option<(u64, String)> = None;
    let mut kept_outlines = BTreeMap::new();

    let (tx, rx) = sync_channel::<(PathBuf, RgbaImage)>(8);
    let rx = Arc::new(Mutex::new(rx));
    let failed: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|scope| -> Result<(), String> {
        for _ in 0..3 {
            let rx = rx.clone();
            let failed = &failed;
            scope.spawn(move || write_frames(&rx, failed));
        }
        let tx = tx;
        for k in warm..=last {
            let (tick, alpha, time) = schedule(k, fps);
            while stepper.world().tick < tick {
                view.before_step(stepper.world());
                if step_noting(stepper, shoot.from..=shoot.to, &mut changes)? {
                    view.reload(renderer.as_deref_mut(), stepper.modules());
                }
            }
            let now = stepper.world().tick;
            while zooms.peek().is_some_and(|z| z.0 <= now) {
                match zooms.next().expect("peeked").1 {
                    Zoom::Steps(steps) => view.zoom(stepper.world(), steps),
                    Zoom::View(v) => view.eye.view = Some(v),
                }
            }
            let world = stepper.world();
            let scene = view.scene(world, alpha, logical, time);
            if k < first {
                continue;
            }
            if k == first {
                kept_outlines = outlines(world, stepper.modules());
            }
            let index = frames.len();
            let file = draw.then(|| format!("frames/{index:05}.png"));
            if let (Some(r), Some(file)) = (renderer.as_deref_mut(), &file) {
                let image = r
                    .render_to_rgba(&scene)
                    .map_err(|e| format!("drawing frame {index} failed: {e}"))?;
                if let Some(e) = failed.lock().expect("not poisoned").take() {
                    return Err(e);
                }
                tx.send((shoot.out.join(file), image))
                    .map_err(|_| "the frame writers stopped".to_owned())?;
            }
            let h = match &hash {
                Some((t, h)) if *t == world.tick => h.clone(),
                _ => {
                    let h = world.hash();
                    hash = Some((world.tick, h.clone()));
                    h
                }
            };
            frames.push(Frame {
                index,
                file,
                time,
                tick: world.tick,
                alpha,
                hash: h,
                camera: cam(&scene.camera, logical, shoot.scale),
                zoom: view.eye.view,
                things: things(
                    view,
                    world,
                    alpha,
                    &scene.camera,
                    logical,
                    shoot.scale,
                    size,
                ),
                lines: scene_lines(&scene),
            });
        }
        drop(tx);
        Ok(())
    })?;
    if let Some(e) = failed.into_inner().expect("not poisoned") {
        return Err(e);
    }
    let manifest = Manifest {
        format: FORMAT,
        fps,
        ticks_per_second: TICKS_PER_SECOND,
        width: shoot.width,
        height: shoot.height,
        scale: shoot.scale,
        drawn: draw,
        adapter: renderer.as_deref().map(Renderer::adapter_info),
        source: shoot.source.clone(),
        builds: builds(&shoot.source),
        changes,
        frames,
        outlines: kept_outlines,
    };
    manifest.write(&shoot.out)?;
    Ok(manifest)
}

/// Which build draws this film, next to the one that recorded the session if it is one.
fn builds(source: &Value) -> Value {
    let this = BuildStamp::current();
    match serde_json::from_value::<BuildStamp>(source["recorded_build"].clone()) {
        Ok(recorded) => {
            let note = source["note"]["number"]
                .as_u64()
                .map_or(String::new(), |n| format!(" --note {n}"));
            let command = source["recording"]
                .as_str()
                .map_or("galaxy-gm film <recording>".to_owned(), |r| {
                    format!("galaxy-gm film {r}{note}")
                });
            galaxy_sim::recording::builds(&recorded, &this, "drew", &command)
        }
        Err(_) => serde_json::json!({
            "drawn": this.label(),
            "note": format!("drawn by {} from the modules as they are now", this.label()),
        }),
    }
}

/// Makes the film folder ready: new, or an old film's, whose frames are cleared.
fn prepare(out: &Path, draw: bool) -> Result<(), String> {
    let frames = out.join("frames");
    if out.exists() {
        let is_film = out.join("film.json").is_file();
        let empty = std::fs::read_dir(out)
            .map_err(|e| format!("can't read {}: {e}", out.display()))?
            .next()
            .is_none();
        if !is_film && !empty {
            return Err(format!(
                "{} has files in it and is not a film; give a new or empty folder",
                out.display()
            ));
        }
        if frames.is_dir() {
            for entry in std::fs::read_dir(&frames)
                .map_err(|e| format!("can't read {}: {e}", frames.display()))?
                .flatten()
            {
                let name = entry.file_name().to_string_lossy().into_owned();
                let ours = name.len() == 9
                    && name.ends_with(".png")
                    && name[..5].bytes().all(|b| b.is_ascii_digit());
                if ours {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    let make = if draw { &frames } else { out };
    std::fs::create_dir_all(make).map_err(|e| format!("can't make {}: {e}", make.display()))
}

fn write_frames(rx: &Mutex<Receiver<(PathBuf, RgbaImage)>>, failed: &Mutex<Option<String>>) {
    loop {
        let job = rx.lock().expect("not poisoned").recv();
        let Ok((path, image)) = job else { return };
        if let Err(e) = image.save_png_fast(&path) {
            failed
                .lock()
                .expect("not poisoned")
                .get_or_insert_with(|| format!("can't write {}: {e}", path.display()));
        }
    }
}

fn cam(camera: &Camera, logical: Vec2, scale: f32) -> Cam {
    Cam {
        eye: camera.eye.to_array(),
        target: camera.target.to_array(),
        view_proj: camera
            .view_proj(logical.x / logical.y.max(1.0))
            .to_cols_array(),
        per_pixel: camera.world_per_pixel(logical) / scale,
    }
}

/// Every drawn thing, and the one the camera follows.
fn things(
    view: &View,
    world: &World,
    alpha: f32,
    camera: &Camera,
    logical: Vec2,
    scale: f32,
    size: (u32, u32),
) -> BTreeMap<String, Thing> {
    let followed = camera_holder(world).map(|(id, _)| id.to_owned());
    world
        .entities
        .iter()
        .filter(|(_, c)| {
            c.contains_key(POSITION) && (c.contains_key(MODEL) || c.contains_key(CAMERA))
        })
        .filter_map(|(id, _)| {
            let pose = view.pose(world, id, alpha)?;
            let screen = camera
                .world_to_screen(pose.point(), logical)
                .map(|p| p * scale);
            let on_screen = screen.is_some_and(|p| {
                p.x >= 0.0 && p.y >= 0.0 && p.x <= size.0 as f32 && p.y <= size.1 as f32
            });
            Some((
                id.clone(),
                Thing {
                    x: pose.x,
                    y: pose.y,
                    height: pose.height,
                    angle: pose.angle,
                    screen: screen.map(|p| p.to_array()),
                    on_screen,
                    followed: followed.as_deref() == Some(id.as_str()),
                    reach: view.reach(world, id),
                    morph: world
                        .component(id, MODEL)
                        .and_then(|m| m.fields.get("morph"))
                        .and_then(Value::as_f64)
                        .filter(|m| m.is_finite() && *m > 0.0)
                        .map(|m| m as f32),
                },
            ))
        })
        .collect()
}

/// Every drawn thing's outline on the plane as collision sees it, in its own frame: a
/// body's circle, else its model's profile, scaled.
fn outlines(world: &World, modules: Vec<&Module>) -> BTreeMap<String, Outline> {
    let shapes = Shapes::of(modules);
    let number = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|v| v.is_finite());
    world
        .entities
        .iter()
        .filter(|(_, c)| c.contains_key(POSITION) && c.contains_key(MODEL))
        .filter_map(|(id, _)| {
            let radius = world
                .component(id, BODY)
                .and_then(|b| number(b.fields.get("radius")))
                .filter(|r| *r > 0.0);
            if let Some(r) = radius {
                let base = vec![r as f32; galaxy_sim::shape::DIRECTIONS];
                return Some((
                    id.clone(),
                    Outline {
                        base,
                        morphed: Vec::new(),
                    },
                ));
            }
            let model = &world.component(id, MODEL)?.fields;
            let profile = shapes.models.get(model.get("model")?.as_str()?)?;
            let scale = number(model.get("scale"))
                .filter(|s| *s > 0.0)
                .unwrap_or(1.0);
            let scaled = |r: &[f64]| r.iter().map(|v| (v * scale) as f32).collect::<Vec<_>>();
            let morphed = if profile.morphed == profile.base {
                Vec::new()
            } else {
                scaled(&profile.morphed)
            };
            Some((
                id.clone(),
                Outline {
                    base: scaled(&profile.base),
                    morphed,
                },
            ))
        })
        .collect()
}

fn scene_lines(scene: &Scene) -> Vec<LineProbe> {
    scene
        .lines
        .iter()
        .map(|l| {
            let (kind, spacing) = match l.style {
                LineStyle::Solid => ("solid", None),
                LineStyle::Dashed { .. } => ("dashed", None),
                LineStyle::Beam => ("beam", None),
                LineStyle::Dotted { .. } => ("dotted", Some(dot_spacing(l))),
            };
            LineProbe {
                kind: kind.into(),
                closed: l.closed,
                alpha: l.color.a,
                width: l.width,
                points: l.points.iter().map(|p| p.to_array()).collect(),
                phase: l.phase,
                spacing,
                dots: dots(l),
            }
        })
        .collect()
}

/// The world distance between dots, as the renderer's dotted shader spaces them.
fn dot_spacing(line: &Line) -> f32 {
    let LineStyle::Dotted { spacing, .. } = line.style else {
        return 0.0;
    };
    let half = line.width * 0.5;
    (spacing.max(line.width) / half).max(2.0) * half
}

/// Where the renderer puts each dot of a dotted line: centred at `(k + 1/2) * spacing`
/// along the pattern, which starts `phase` in at the first point; and how much of each
/// the end fade leaves.
fn dots(line: &Line) -> Vec<[f32; 4]> {
    let LineStyle::Dotted { fade, .. } = line.style else {
        return Vec::new();
    };
    if !(line.width.is_finite() && line.width > 0.0) || line.points.len() < 2 {
        return Vec::new();
    }
    let spacing = dot_spacing(line);
    let lengths: Vec<f32> = line
        .points
        .windows(2)
        .map(|w| (w[1] - w[0]).length())
        .collect();
    let total: f32 = lengths.iter().sum();
    let mut out = Vec::new();
    let mut k = ((line.phase / spacing) - 0.5).ceil();
    loop {
        let along = (k + 0.5) * spacing - line.phase;
        k += 1.0;
        if along < 0.0 {
            continue;
        }
        if along > total || out.len() > 10_000 {
            break;
        }
        let mut left = along;
        let mut at = *line.points.last().expect("two points");
        for (i, len) in lengths.iter().enumerate() {
            if left <= *len {
                let t = if *len > 0.0 { left / len } else { 0.0 };
                at = line.points[i] + (line.points[i + 1] - line.points[i]) * t;
                break;
            }
            left -= len;
        }
        let x = ((total - along) / fade.max(1e-6)).clamp(0.0, 1.0);
        let visible = x * x * (3.0 - 2.0 * x);
        out.push([at.x, at.y, at.z, visible]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use galaxy_render::glam::Vec3;
    use galaxy_render::{Color, Line};

    #[test]
    fn frames_land_between_ticks_as_the_window_draws_them() {
        assert_eq!(schedule(0, 60), (0, 0.0, 0.0));
        assert_eq!(schedule(1, 60), (0, 0.5, 1.0 / 60.0));
        assert_eq!(schedule(2, 60), (1, 0.0, 2.0 / 60.0));
        assert_eq!(schedule(5, 60).0, 2);
        // At 30 fps every frame is on a tick; at 144, most are between.
        assert_eq!(schedule(7, 30), (7, 0.0, 7.0 / 30.0));
        let (tick, alpha, _) = schedule(10, 144);
        assert_eq!(tick, 2);
        assert!((alpha - (300 % 144) as f32 / 144.0).abs() < 1e-6);
    }

    #[test]
    fn dots_sit_where_the_shader_draws_them() {
        let line = Line::path(
            vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)],
            0.5,
            Color::WHITE,
        )
        .dotted(2.0, 1.0)
        .with_phase(0.5);
        let d = dots(&line);
        // Centres at (k + 1/2) * 2 - 0.5 along: 0.5, 2.5, 4.5, 6.5, 8.5.
        let xs: Vec<f32> = d.iter().map(|p| p[0]).collect();
        assert_eq!(xs, [0.5, 2.5, 4.5, 6.5, 8.5]);
        assert_eq!(d[0][3], 1.0);
        // The last is 1.5 from the end, past the fade of 1: whole.
        assert_eq!(d[4][3], 1.0);
        let faded = Line::path(
            vec![Vec3::ZERO, Vec3::new(9.0, 0.0, 0.0)],
            0.5,
            Color::WHITE,
        )
        .dotted(2.0, 1.0)
        .with_phase(0.5);
        let d = dots(&faded);
        assert!(d[4][3] < 0.6 && d[4][3] > 0.4, "{}", d[4][3]);
    }
}

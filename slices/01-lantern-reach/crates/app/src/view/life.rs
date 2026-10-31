//! A station that lives (#47, 4.1; the station concept's "Dreaming ahead"): what the
//! docked station shows of what is going on in it. Wherever it can, motion is a real
//! thing happening in the simulation:
//!
//! - **Stock piles** at the market grow and shrink with each market line's stock.
//! - **The cargo crane** lifts what the player just sold from their berth to the
//!   stalls, set off by the `sell` fact and played over the ticks after it.
//! - **A crowd** gathers at the notice wall (or the office) when notices or rumors have
//!   landed here since the player's last docking.
//! - **The lamplighter** lights one more lamp per station pulse (the economy's activity
//!   tally), walking to it when a ship docks: events, never a clock.
//! - **Workers and sparks** stand at a part that is `building`.
//! - **Residents** walk the pier, more of them as the station's pulses add up.
//! - The economy's **own craft** arrive at and leave berths ([`super::View::world_scene`]);
//!   filler craft from the theme only make up the gap to its calm cap.
//! - Small idle loops (lamp flicker, steam, the banner, signal lamps) run on view time.
//!
//! The station's theme ([`super::themes`]) decides the props, the residents and their
//! paint, and caps how much moves (`calm`). Game master happenings
//! ([`happenings::active`]) add what their `show` entries ask for. Everything here is
//! view only: it reads the world and history and changes neither.
//!
//! Keeping it calm: ambient things are small, slow and drawn with a light outline, so
//! they sit under the modules. Nothing flashes. Nothing that moves stands on a module's
//! click target or its sign ([`Clear`]); a walker paces only the clear stretches of its
//! path. [`LifeLevel::Quiet`] (Q, or the PAUSED card's button) keeps what the numbers
//! say (stock, lamps, the crane with your sale, the crowd) and stills the rest.

use std::path::Path;

use galaxy_contract::parts::{PartState, RoomKind, StationPart};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::Channel;
use galaxy_core::History;
use render::glam::{Vec2 as G2, Vec3};
use render::kit::life::{CRATE_SIZE, LAMP_LIGHT, RESIDENT_HEIGHT, huddle};
use render::kit::{PartKind, StationPalette, StationRecipe};
use render::{ArtBible, Color, Glow, Line, PointLight};
use serde::{Deserialize, Serialize};

use super::themes::{Prop, Resident, Spine, Theme};
use super::{DOCK_TILT, g, modules, seed_of, station_transform};

/// How much ambient life the docked station shows: a setting the player keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifeLevel {
    /// Everything the theme allows, within its calm caps.
    #[default]
    Full,
    /// What the numbers say (stock, lamps, your sale on the crane, a crowd) and nothing
    /// else: no walkers, no filler craft, no steam or sparks, nothing swaying.
    Quiet,
}

impl LifeLevel {
    pub fn toggled(self) -> Self {
        match self {
            LifeLevel::Full => LifeLevel::Quiet,
            LifeLevel::Quiet => LifeLevel::Full,
        }
    }

    /// The PAUSED card's button: the setting, then the key and what it does.
    pub fn label(self) -> &'static str {
        match self {
            LifeLevel::Full => "Station life: Full · Q quiets it",
            LifeLevel::Quiet => "Station life: Quiet · Q livens it",
        }
    }
}

/// The view's settings file, next to the saves.
pub const SETTINGS_FILE: &str = "view.toml";

/// The view's own settings, kept across saves in [`SETTINGS_FILE`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewSettings {
    /// Q in the game, or the PAUSED card's button, switches it.
    pub life: LifeLevel,
}

impl ViewSettings {
    /// Reads the file: missing means the defaults; unreadable means the defaults and a
    /// note saying why.
    pub fn load(path: &Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Self::default(), None),
            Err(e) => (Self::default(), Some(format!("{}: {e}", path.display()))),
            Ok(text) => match toml::from_str(&text) {
                Ok(s) => (s, None),
                Err(e) => (
                    Self::default(),
                    Some(format!("{}: {e}; using the defaults", path.display())),
                ),
            },
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let body = toml::to_string(self).map_err(|e| e.to_string())?;
        let text = format!(
            "# The game's view settings. `life` is \"full\" or \"quiet\": how much ambient life\n\
             # a docked station shows. Q in the game, or the PAUSED card's button, switches it.\n{body}"
        );
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Game master happenings, read through worker B's one call. Until
/// `galaxy_progress::happenings` lands this stand-in has the same signature and returns
/// nothing; the merge replaces it with `use galaxy_progress::happenings;`.
pub mod happenings {
    use galaxy_contract::prelude::*;

    /// The part of a happening the view reads: its `show` entries.
    pub struct Happening {
        pub show: Vec<String>,
    }

    /// The happenings running at `station` now.
    pub fn active<'a>(_world: &'a WorldState, _station: &StationId) -> Vec<&'a Happening> {
        Vec::new()
    }
}

/// What the running happenings ask the station to show. Entries the view doesn't know
/// are ignored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shows {
    /// `banner_string:lamps`: a string of lamps hung over the quay.
    pub lamp_string: bool,
    /// `crowd:<anchor>`: a crowd at a part (by ID) or a kit anchor (by name).
    pub crowds: Vec<String>,
    /// `residents:+N`: N more residents walking (still within the calm cap).
    pub residents: u32,
}

impl Shows {
    pub fn read<'a>(entries: impl IntoIterator<Item = &'a str>) -> Self {
        let mut s = Shows::default();
        for e in entries {
            match e.split_once(':') {
                Some(("banner_string", "lamps")) => s.lamp_string = true,
                Some(("crowd", at)) if !at.is_empty() => s.crowds.push(at.to_owned()),
                Some(("residents", n)) => {
                    if let Ok(n) = n.trim_start_matches('+').parse::<u32>() {
                        s.residents = s.residents.saturating_add(n.min(16));
                    }
                }
                _ => {}
            }
        }
        s
    }

    pub fn of(world: &WorldState, station: &StationId) -> Self {
        Self::read(
            happenings::active(world, station)
                .into_iter()
                .flat_map(|h| h.show.iter().map(String::as_str)),
        )
    }
}

/// Where nothing that moves may stand: every module's click target and the sign under
/// it, as rectangles on the plane. Laid out for the docked overview's framing (at the
/// design size); the room view comes closer and the cradle tilts less, so they stay
/// clear there too.
#[derive(Clone, Debug, Default)]
pub struct Clear {
    rects: Vec<(G2, G2)>,
    /// How far up the plane a thing's top shows for each unit of height (the tilt).
    lean: f32,
}

/// A click target's least radius, in canvas pixels ([`crate::ui::station`]).
const TARGET_MIN_PX: f32 = 26.0;
/// The share of the screen's height a docked station fills ([`crate::frame`]).
const DOCK_FILL: f32 = 0.6;

impl Clear {
    /// The keep-clear rectangles around `station`'s modules.
    pub fn at(world: &WorldState, station: &StationId) -> Self {
        let lean = DOCK_TILT.to_radians().tan();
        let Some(st) = world.stations.get(station) else {
            return Self::default();
        };
        let recipe = galaxy_core::parts::recipe_of(world, st);
        let to_world = station_transform(st, &recipe);
        let centre = to_world.w_axis.truncate().truncate();
        let parts: Vec<(&StationPart, G2, f32)> = galaxy_core::parts::of_station(world, station)
            .filter_map(|p| modules::module_point(world, p).map(|(at, r)| (p, at, r)))
            .collect();
        let mut reach = recipe.footprint() * to_world.x_axis.truncate().length();
        for (_, p, r) in &parts {
            reach = reach.max(p.distance(centre) + r);
        }
        // World units per canvas pixel in the overview, at the design height.
        let wpp = (reach * 2.0 / DOCK_FILL).max(160.0) / crate::ui::DESIGN.y;
        let squash = 1.0 / DOCK_TILT.to_radians().cos();
        let mut rects = Vec::new();
        for (part, p, r) in parts {
            let half = r.max(TARGET_MIN_PX * wpp);
            // The target is centred where the module's middle shows, a little up the plane.
            let mid = p + G2::new(0.0, modules::LIFT * 0.5 * lean);
            let hy = half * squash;
            rects.push((mid - G2::new(half, hy), mid + G2::new(half, hy)));
            // The sign hangs under it ([`crate::ui::station::signs`]): its word, 24 px tall.
            let word = match part.state {
                PartState::Open => part.title().to_owned(),
                _ => format!("{}  ·  Building", part.title()),
            };
            let w = (crate::ui::text_width(&word, 14.0) + 20.0) * 0.5 * wpp;
            let top = mid.y - 0.9 * hy;
            let bottom = top - 26.0 * wpp * squash;
            rects.push((G2::new(mid.x - w, bottom), G2::new(mid.x + w, top)));
        }
        Self { rects, lean }
    }

    /// Whether a thing standing at `foot` (raised `foot.z` off the plane), `height` world
    /// units tall, keeps `pad` clear of every module and sign as the tilted camera shows
    /// it.
    pub fn ok(&self, foot: Vec3, height: f32, pad: f32) -> bool {
        let base = foot.truncate() + G2::new(0.0, foot.z * self.lean);
        let top = base + G2::new(0.0, height * self.lean);
        let lo = base.min(top) - G2::splat(pad);
        let hi = base.max(top) + G2::splat(pad);
        !self
            .rects
            .iter()
            .any(|(a, b)| lo.x < b.x && a.x < hi.x && lo.y < b.y && a.y < hi.y)
    }
}

/// What a figure is doing there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Walker,
    Lamplighter,
    Crowd,
    Worker,
}

/// A resident, placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Figure {
    pub resident: Resident,
    pub role: Role,
    /// Feet, in world units.
    pub at: Vec3,
    /// Radians, the way they face.
    pub facing: f32,
    pub moving: bool,
}

/// One unit of stock in a pile.
#[derive(Clone, Debug, PartialEq)]
pub struct Stock {
    pub ware: WareId,
    pub at: Vec3,
    pub turn: f32,
}

/// A lamp post.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lamp {
    pub at: Vec3,
    pub lit: bool,
}

/// The cargo crane and what hangs from it.
#[derive(Clone, Debug, PartialEq)]
pub struct Crane {
    /// The mast's foot.
    pub at: Vec3,
    /// Which way the jib points, radians.
    pub yaw: f32,
    pub height: f32,
    pub reach: f32,
    /// What it carries (the ware just sold) and where that hangs.
    pub load: Option<(WareId, Vec3)>,
}

/// A filler craft making up the theme's traffic.
#[derive(Clone, Debug, PartialEq)]
pub struct Filler {
    pub hull: HullId,
    pub faction: FactionId,
    pub at: G2,
    pub heading: f32,
}

/// Everything the life layer draws at one station this frame, in world space.
#[derive(Clone, Debug, Default)]
pub struct LifeFrame {
    pub figures: Vec<Figure>,
    pub stock: Vec<Stock>,
    pub lamps: Vec<Lamp>,
    pub crane: Option<Crane>,
    /// Banner poles' feet.
    pub poles: Vec<Vec3>,
    pub craft: Vec<Filler>,
    pub glows: Vec<Glow>,
    pub lines: Vec<Line>,
    pub lights: Vec<PointLight>,
    /// Steam, sparks and signal lamps drawn (within the theme's `max_particles`).
    pub particles: u32,
    /// World units per station unit (a resident is [`RESIDENT_HEIGHT`] station units).
    pub scale: f32,
}

impl LifeFrame {
    /// Every moving thing's foot and height in world units, for keep-clear checks.
    pub fn movers(&self) -> Vec<(Vec3, f32)> {
        let mut out: Vec<(Vec3, f32)> = self
            .figures
            .iter()
            .filter(|f| f.moving)
            .map(|f| (f.at, RESIDENT_HEIGHT * self.scale))
            .collect();
        if let Some((_, at)) = self.crane.as_ref().and_then(|c| c.load.as_ref()) {
            out.push((*at, CRATE_SIZE * self.scale));
        }
        out.extend(
            self.craft
                .iter()
                .map(|c| (c.at.extend(super::SHIP_HOVER), 0.0)),
        );
        out
    }

    /// How many walk the pier (the crowd, the workers and the lamplighter aside).
    pub fn walkers(&self) -> usize {
        self.figures
            .iter()
            .filter(|f| f.role == Role::Walker)
            .count()
    }
}

/// What [`frame`] reads.
pub struct LifeInput<'a> {
    pub world: &'a WorldState,
    pub history: &'a History,
    pub station: &'a StationId,
    pub theme: &'a Theme,
    pub level: LifeLevel,
    /// Presentation seconds, for idle loops only.
    pub time: f32,
}

/// The quay's radius (station units): the walk along the modules' front.
const QUAY_R: f32 = 17.0;
/// How long the crane takes over a sale, and the lamplighter over a lamp, in ticks.
const CARRY_TICKS: u64 = 6 * galaxy_contract::TICKS_PER_SECOND as u64;
const WALK_TICKS: u64 = 4 * galaxy_contract::TICKS_PER_SECOND as u64;
/// Walking pace, station units a second: an unhurried stroll.
const STROLL: f32 = 1.4;
/// The crane's mast, station units.
const CRANE_HEIGHT: f32 = 9.0;

/// A point `r` station units from `c` at `deg` degrees.
fn polar(c: G2, s: f32, deg: f32, r: f32) -> G2 {
    c + G2::from_angle(deg.to_radians()) * r * s
}

fn smooth(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

fn heading(d: G2) -> f32 {
    d.y.atan2(d.x)
}

/// The station's geometry for life: its decks, its scale and its parts.
struct Site<'a> {
    st: &'a Station,
    recipe: StationRecipe,
    /// The station-space origin (the hub) on the plane.
    c: G2,
    /// World units per station unit.
    s: f32,
    /// The quay's span, degrees (from, to), round the modules' side of the hub.
    arc: (f32, f32),
    parts: Vec<&'a StationPart>,
    /// The docking ring, the pier where ships berth: centre, radius and deck height, in
    /// world units.
    ring: Option<(G2, f32, f32)>,
    /// The hub's top deck: radius and height, in world units.
    deck: (f32, f32),
}

impl<'a> Site<'a> {
    fn new(world: &'a WorldState, station: &'a StationId) -> Option<Self> {
        let st = world.stations.get(station)?;
        let recipe = galaxy_core::parts::recipe_of(world, st);
        let to_world = station_transform(st, &recipe);
        let c = to_world.w_axis.truncate().truncate();
        let s = to_world.x_axis.truncate().length();
        let parts: Vec<&StationPart> = galaxy_core::parts::of_station(world, station).collect();
        let angles: Vec<f32> = parts
            .iter()
            .filter_map(|p| modules::module_point(world, p))
            .map(|(p, _)| heading(p - c).to_degrees().rem_euclid(360.0))
            .collect();
        let arc = if angles.is_empty() {
            (120.0, 240.0)
        } else {
            let lo = angles.iter().copied().fold(f32::MAX, f32::min);
            let hi = angles.iter().copied().fold(f32::MIN, f32::max);
            (lo - 18.0, hi + 18.0)
        };
        let ring = recipe.parts.iter().find_map(|p| match p.kind {
            PartKind::DockingRing { radius, tube, .. } => Some((
                polar(c, s, p.at.angle, p.at.distance),
                radius * p.at.scale * s,
                (p.at.z + tube * 0.5 * p.at.scale) * s,
            )),
            _ => None,
        });
        let deck = recipe
            .parts
            .iter()
            .find_map(|p| match p.kind {
                PartKind::Hub {
                    radius,
                    height,
                    tiers,
                    ..
                } => {
                    let shrink = 1.0 - 0.22 * tiers.saturating_sub(1).min(3) as f32;
                    Some((
                        radius * shrink * 0.88 * p.at.scale * s,
                        (p.at.z + height * 0.5 * p.at.scale) * s,
                    ))
                }
                _ => None,
            })
            .unwrap_or((4.0 * s, 2.0 * s));
        Some(Self {
            st,
            recipe,
            c,
            s,
            arc,
            parts,
            ring,
            deck,
        })
    }

    /// `n` points along the quay at `r` station units.
    fn quay(&self, r: f32, n: usize) -> Vec<Vec3> {
        (0..n)
            .map(|i| {
                let k = i as f32 / (n - 1).max(1) as f32;
                polar(self.c, self.s, self.arc.0 + (self.arc.1 - self.arc.0) * k, r).extend(0.0)
            })
            .collect()
    }

    /// A circle of `n` points round `centre` at `r` world units, `z` up.
    fn circle(centre: G2, r: f32, z: f32, n: usize) -> Vec<Vec3> {
        (0..=n)
            .map(|i| (centre + G2::from_angle(i as f32 / n as f32 * std::f32::consts::TAU) * r).extend(z))
            .collect()
    }

    fn part(&self, room: RoomKind) -> Option<&'a StationPart> {
        self.parts.iter().copied().find(|p| p.room == room)
    }
}

/// The stretches of `path` where a figure keeps clear, each at least three samples.
fn clear_runs(path: &[Vec3], clear: &Clear, height: f32, pad: f32) -> Vec<Vec<Vec3>> {
    let mut runs: Vec<Vec<Vec3>> = Vec::new();
    let mut run = Vec::new();
    for p in path {
        if clear.ok(*p, height, pad) {
            run.push(*p);
        } else if !run.is_empty() {
            runs.push(std::mem::take(&mut run));
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs.retain(|r| r.len() >= 3);
    runs
}

fn length(run: &[Vec3]) -> f32 {
    run.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Where along `run` a walker is after walking `dist` there and back: the point and the
/// way it faces.
fn along(run: &[Vec3], dist: f32) -> (Vec3, f32) {
    let len = length(run).max(1e-3);
    let t = dist.rem_euclid(2.0 * len);
    let (mut d, back) = if t > len {
        (2.0 * len - t, true)
    } else {
        (t, false)
    };
    for w in run.windows(2) {
        let seg = w[0].distance(w[1]);
        if d <= seg || seg <= 0.0 {
            let k = if seg > 0.0 { d / seg } else { 0.0 };
            let dir = if back { w[0] - w[1] } else { w[1] - w[0] };
            return (w[0].lerp(w[1], k), heading(dir.truncate()));
        }
        d -= seg;
    }
    (run[run.len() - 1], 0.0)
}

/// The station's pulses so far, whole.
fn pulses(world: &WorldState, station: &StationId) -> u64 {
    world
        .economy
        .stations
        .get(station)
        .map_or(0, |t| u64::try_from(t.pulses_milli.max(0) / 1000).unwrap_or(0))
}

/// The ticks of the player's last two dockings at `station`, newest first.
fn player_dockings(world: &WorldState, history: &History, station: &StationId) -> Vec<Tick> {
    let Some(ship) = world.player.ship.as_ref() else {
        return Vec::new();
    };
    let dock = FactKind::from_static("dock");
    history
        .facts()
        .iter()
        .rev()
        .filter(|f| {
            f.kind == dock
                && f.subjects.ship.as_ref() == Some(ship)
                && f.subjects.station.as_ref() == Some(station)
        })
        .take(2)
        .map(|f| f.tick)
        .collect()
}

/// Notices and rumors that landed at `station` since the player's last docking there
/// (on a first visit, since this one began).
pub fn news_since_last_docking(world: &WorldState, history: &History, station: &StationId) -> u32 {
    let dockings = player_dockings(world, history, station);
    let since = dockings
        .get(1)
        .or(dockings.first())
        .copied()
        .unwrap_or(Tick(0));
    let notices = world
        .talk
        .lines
        .values()
        .filter(|l| {
            l.tick >= since
                && l.line.place.as_ref() == Some(station)
                && match l.line.channel {
                    Channel::Chronicle | Channel::MapNote => true,
                    Channel::DockTalk => l.line.speaker.is_none(),
                }
        })
        .count();
    let rumors = world
        .rumors
        .values()
        .filter(|r| r.heard_at.contains(station) && !r.heard_by_player)
        .count();
    u32::try_from(notices + rumors).unwrap_or(u32::MAX)
}

/// How many crates a market line's stock makes: four at its target, at most eight.
pub fn crates_for(line: &MarketLine) -> u32 {
    let n = if line.target > 0 {
        (4.0 * line.stock as f32 / line.target as f32).round()
    } else {
        (line.stock as f32 / 25.0).round()
    };
    (n as u32).min(8)
}

/// Where a pile's `i`th crate sits, in station units from the pile's middle: two by
/// two on the deck, then a second layer on top.
fn crate_slot(i: u32) -> Vec3 {
    let layer = i / 4;
    let k = i % 4;
    let shift = if layer > 0 { 0.25 } else { 0.0 };
    Vec3::new(
        ((k % 2) as f32 - 0.5) * 1.4 + shift,
        ((k / 2) as f32 - 0.5) * 1.4 + shift * 0.5,
        layer as f32 * CRATE_SIZE,
    )
}

/// Spots round `at` (a module), towards the hub first: (spot, the way it faces).
fn spots_round(site: &Site, at: G2, out: f32) -> Vec<(G2, G2)> {
    let inward = (site.c - at).normalize_or(G2::Y);
    [0.0f32, 35.0, -35.0, 70.0, -70.0, 110.0, -110.0, 150.0, -150.0]
        .into_iter()
        .flat_map(|a| {
            let dir = G2::from_angle(a.to_radians()).rotate(inward);
            [1.0, 1.3, 1.7].map(|k| (at + dir * out * k, dir))
        })
        .collect()
}

/// The life at the player's docked station this frame. Pure: the same world, history,
/// theme, level and time give the same frame.
pub fn frame(input: &LifeInput) -> LifeFrame {
    let LifeInput {
        world,
        history,
        station,
        theme,
        level,
        time,
    } = *input;
    let Some(site) = Site::new(world, station) else {
        return LifeFrame::default();
    };
    let clear = Clear::at(world, station);
    let s = site.s;
    let calm = theme.calm;
    let full = level == LifeLevel::Full;
    let now = world.meta.tick.0;
    let shows = Shows::of(world, station);
    let pulses = pulses(world, station);
    let pal = palette_for(theme, Some(site.st));
    let fig_h = RESIDENT_HEIGHT * s;
    let pad = 0.5 * s;
    let mut out = LifeFrame {
        scale: s,
        ..LifeFrame::default()
    };
    let mut particles = calm.max_particles;
    let mut spend = |n: u32| -> u32 {
        let n = n.min(particles);
        particles -= n;
        n
    };
    let (deck_r, deck_z) = site.deck;

    // Lamps round the hub's top deck, lit one more per station pulse.
    if theme.has(Prop::LampString) {
        let k = theme.lamps as usize;
        let spots: Vec<Vec3> = (0..k)
            .map(|i| {
                let a = 90.0 + i as f32 / k as f32 * 360.0;
                polar(site.c, 1.0, a, deck_r * 0.92).extend(deck_z)
            })
            .filter(|p| clear.ok(*p, 4.7 * s, pad))
            .collect();
        let n = spots.len() as u64;
        if n > 0 {
            // A ship's docking is a pulse: the lamplighter walks to the next lamp and
            // lights it on arrival.
            let walk = if full && pulses > 0 {
                history
                    .last(&HistoryQuery {
                        kind: Some(FactKind::from_static("dock")),
                        station: Some(station.clone()),
                        since: Some(Tick(now.saturating_sub(WALK_TICKS))),
                        ..HistoryQuery::default()
                    })
                    .map(|f| now.saturating_sub(f.tick.0) as f32 / WALK_TICKS as f32)
            } else {
                None
            };
            let lit = pulses.min(n);
            let newest = (pulses.max(1) - 1) % n;
            for (i, p) in spots.iter().enumerate() {
                let i = i as u64;
                let fresh = i == newest && walk.is_some_and(|k| k < 1.0);
                let on = i < lit && !fresh;
                out.lamps.push(Lamp { at: *p, lit: on });
                if on {
                    let top = *p + LAMP_LIGHT * s;
                    let flicker = if full {
                        0.92 + 0.08 * (time * 1.7 + i as f32 * 1.3).sin()
                    } else {
                        1.0
                    };
                    out.glows.push(Glow {
                        position: top,
                        radius: 1.5 * s,
                        color: pal.glow.with_alpha(0.4 * flicker),
                    });
                    out.lights.push(PointLight {
                        position: top + Vec3::Z * s,
                        color: pal.glow.scale(0.55),
                        radius: 8.0 * s,
                    });
                }
            }
            // The lamplighter: at the newest lamp, or on the way to it round the deck.
            let to = spots[newest as usize];
            let from = spots[((newest + n - 1) % n) as usize];
            let (at, moving) = match walk {
                Some(k) if n > 1 && k < 1.0 => {
                    let (a0, a1) = (heading((from - site.c.extend(0.0)).truncate()), heading((to - site.c.extend(0.0)).truncate()));
                    let mut turn = (a1 - a0).rem_euclid(std::f32::consts::TAU);
                    if turn > std::f32::consts::PI {
                        turn -= std::f32::consts::TAU;
                    }
                    let a = a0 + turn * smooth(k);
                    ((site.c + G2::from_angle(a) * deck_r * 0.92).extend(deck_z), true)
                }
                _ => (to, false),
            };
            let stand = at.truncate() + (site.c - at.truncate()).normalize_or(G2::Y) * 0.9 * s;
            let stand = stand.extend(deck_z);
            if clear.ok(stand, fig_h, pad) {
                out.figures.push(Figure {
                    resident: theme.lamplighter(),
                    role: Role::Lamplighter,
                    at: stand,
                    facing: heading(to.truncate() - stand.truncate()),
                    moving,
                });
            }
        }
    }

    // A lamp string over the pier for a happening.
    if shows.lamp_string {
        let pts: Vec<Vec3> = match site.ring {
            Some((rc, rr, rz)) => Site::circle(rc, rr, 0.0, 36)
                .into_iter()
                .enumerate()
                .map(|(i, p)| p.truncate().extend(rz + (4.5 + 0.8 * ((i % 4) as f32 - 1.5).abs()) * s))
                .collect(),
            None => site.quay(QUAY_R, 36),
        };
        out.lines
            .push(Line::path(pts.clone(), 0.15 * s, pal.trim.with_alpha(0.6)));
        for p in pts.iter().step_by(3) {
            out.glows.push(Glow {
                position: *p,
                radius: 0.9 * s,
                color: pal.glow.with_alpha(0.45),
            });
        }
    }

    // Stock piles in a small yard by the market, one pile per market line.
    let mut yard = None;
    if let (Some(market), Some(lines)) = (
        site.part(RoomKind::Market),
        world.markets.get(station).map(|m| &m.lines),
    ) && let Some((at, r)) = modules::module_point(world, market)
    {
        let piles: Vec<(&WareId, u32)> = lines.iter().map(|(w, l)| (w, crates_for(l))).collect();
        let cols = piles.len().div_ceil(2).max(1);
        let pitch = 3.0;
        let placed = spots_round(&site, at, r + 2.2 * s)
            .into_iter()
            .find_map(|(spot, dir)| {
                let side = G2::new(-dir.y, dir.x);
                let middle = |i: usize| {
                    let col = (i / 2) as f32 - (cols as f32 - 1.0) * 0.5;
                    let row = (i % 2) as f32 - 0.5;
                    spot + side * col * pitch * s + dir * row * pitch * s
                };
                // The whole yard keeps clear, full or empty.
                let corners_clear = (0..cols * 2)
                    .all(|i| clear.ok(middle(i).extend(0.0), 2.0 * CRATE_SIZE * s, pad + s));
                corners_clear.then(|| {
                    let crates: Vec<Stock> = piles
                        .iter()
                        .enumerate()
                        .flat_map(|(i, (ware, n))| {
                            let m = middle(i);
                            (0..*n).map(move |k| {
                                let c = crate_slot(k);
                                Stock {
                                    ware: (*ware).clone(),
                                    at: (m + side * c.x * s + dir * c.y * s).extend(c.z * s),
                                    turn: heading(side) + (k as f32 * 0.37).sin() * 0.12,
                                }
                            })
                        })
                        .collect();
                    (spot, crates)
                })
            });
        if let Some((spot, crates)) = placed {
            out.stock = crates;
            yard = Some(spot);
        }
    }
    if let Some(spot) = yard {
        out.crane = crane(world, history, &site, &clear, spot, now);
    }

    // A crowd at the notice wall (or the office) when news has landed.
    let news = news_since_last_docking(world, history, station);
    let wall = site
        .part(RoomKind::NoticeWall)
        .or_else(|| site.part(RoomKind::Dockmaster));
    let mut crowds: Vec<(G2, f32, u32)> = Vec::new();
    if news > 0
        && let Some((at, r)) = wall.and_then(|p| modules::module_point(world, p))
    {
        let n = (2 + 2 * news).min(calm.max_crowd);
        crowds.push((at, r, if full { n } else { n.min(2) }));
    }
    for anchor in &shows.crowds {
        if let Some((at, r)) = anchor_point(world, &site, anchor) {
            crowds.push((at, r, calm.max_crowd.div_ceil(2).max(2)));
        }
    }
    for (at, r, n) in crowds {
        let ring = huddle(n as usize, 1.2);
        let spot = spots_round(&site, at, r + 2.0 * s)
            .into_iter()
            .find(|(spot, _)| {
                ring.iter()
                    .all(|o| clear.ok((*spot + o.truncate() * s).extend(0.0), fig_h, pad))
            });
        if let Some((spot, _)) = spot {
            for (i, o) in ring.iter().enumerate() {
                let p = spot + o.truncate() * s;
                let sway = if full {
                    0.15 * (time * 0.6 + i as f32 * 1.7).sin()
                } else {
                    0.0
                };
                out.figures.push(Figure {
                    resident: theme.residents[i % theme.residents.len()],
                    role: Role::Crowd,
                    at: p.extend(0.0),
                    facing: heading(at - p) + sway,
                    moving: full,
                });
            }
        }
    }

    // Workers and sparks at a part being built.
    for part in site.parts.iter().filter(|p| p.state == PartState::Building) {
        let Some((at, r)) = modules::module_point(world, part) else {
            continue;
        };
        let mut placed = 0;
        for (spot, _) in spots_round(&site, at, r + 1.4 * s) {
            if placed == 3 {
                break;
            }
            let foot = spot.extend(0.0);
            if !clear.ok(foot, fig_h, pad)
                || out
                    .figures
                    .iter()
                    .any(|f| f.at.truncate().distance(spot) < 1.6 * s)
            {
                continue;
            }
            let bob = if full {
                0.08 * s * (time * 2.1 + placed as f32 * 2.0).sin().max(0.0)
            } else {
                0.0
            };
            out.figures.push(Figure {
                resident: theme.builder(),
                role: Role::Worker,
                at: foot + Vec3::Z * bob,
                facing: heading(at - spot),
                moving: full,
            });
            if full && spend(1) == 1 {
                let hands = spot + (at - spot).normalize_or_zero() * 0.8 * s;
                let glint = 0.25 + 0.12 * (time * 3.1 + placed as f32 * 1.9).sin();
                out.glows.push(Glow {
                    position: hands.extend(1.3 * s),
                    radius: 0.8 * s,
                    color: Color::hex(0xffd9a0).with_alpha(glint),
                });
            }
            placed += 1;
        }
    }

    // Residents walking: more as the pulses add up, never past the cap.
    if full {
        let want = (2 + pulses / 3 + u64::from(shows.residents)).min(u64::from(calm.max_walkers));
        let mut runs: Vec<Vec<Vec3>> = Vec::new();
        for spine in &theme.spine {
            for path in walk_paths(world, &site, *spine) {
                runs.extend(clear_runs(&path, &clear, fig_h, pad));
            }
        }
        let kinds: Vec<Resident> = {
            let k: Vec<Resident> = theme
                .residents
                .iter()
                .copied()
                .filter(|r| !matches!(r, Resident::Lamplighter | Resident::Welder))
                .collect();
            if k.is_empty() {
                theme.residents.clone()
            } else {
                k
            }
        };
        if !runs.is_empty() {
            for i in 0..want as usize {
                let run = &runs[i % runs.len()];
                let seed = seed_of(&format!("{station}.walker.{i}"));
                let phase = (seed % 1000) as f32 / 1000.0 * length(run) * 2.0;
                let pace = STROLL * s * (0.8 + (seed % 7) as f32 * 0.06);
                let (p, facing) = along(run, phase + time * pace);
                out.figures.push(Figure {
                    resident: kinds[i % kinds.len()],
                    role: Role::Walker,
                    at: p,
                    facing,
                    moving: true,
                });
            }
        }
    }

    // Idle loops: steam, greenhouse light, signal lamps, the banner.
    if full && theme.has(Prop::Steam) {
        let n = spend(4);
        for i in 0..n {
            let k = (time / 5.0 + i as f32 / n.max(1) as f32).fract();
            let drift = G2::from_angle(i as f32 * 2.1) * 1.2 * s * k;
            out.glows.push(Glow {
                position: (site.c + drift).extend(deck_z + (1.0 + k * 5.0) * s),
                radius: (0.8 + 1.6 * k) * s,
                color: Color::hex(0xfff4e0).with_alpha(0.14 * (1.0 - k)),
            });
        }
    }
    if theme.has(Prop::GreenhouseGlow) {
        for p in &site.recipe.parts {
            if let PartKind::Greenhouse { radius, .. } = p.kind {
                let at = polar(site.c, s, p.at.angle, p.at.distance);
                let breathe = if full {
                    0.9 + 0.1 * (time * 0.5).sin()
                } else {
                    1.0
                };
                out.glows.push(Glow {
                    position: at.extend(2.0 * s),
                    radius: radius * 1.5 * s,
                    color: pal.leaf.with_alpha(0.18 * breathe),
                });
            }
        }
    }
    if theme.has(Prop::SignalLamps) {
        let n = spend(5);
        for i in 0..n {
            let p = polar(site.c, 1.0, 200.0 + i as f32 * 32.0, deck_r * 0.7);
            let a = if full {
                0.2 + 0.16 * smooth(0.5 + 0.5 * (time * 0.7 - i as f32 * 0.8).sin())
            } else {
                0.28
            };
            out.glows.push(Glow {
                position: p.extend(deck_z + 0.6 * s),
                radius: 0.8 * s,
                color: pal.accent.lerp(pal.glow, 0.5).with_alpha(a),
            });
        }
    }
    if theme.has(Prop::Banner)
        && let Some(owner) = site.st.owner.as_ref()
    {
        let foot = polar(site.c, 1.0, 315.0, deck_r * 0.55).extend(deck_z);
        let h = 7.0 * s;
        if clear.ok(foot, h, pad) {
            out.poles.push(foot);
            let color = super::recipes::faction_color(owner);
            let fly = G2::from_angle(0.35);
            let pts: Vec<Vec3> = (0..6)
                .map(|k| {
                    let k = k as f32;
                    let wave = if full {
                        0.35 * s * (time * 1.1 - k * 0.9).sin() * (k / 5.0)
                    } else {
                        0.0
                    };
                    (foot.truncate() + fly * k * 0.7 * s).extend(foot.z + h - 0.6 * s + wave)
                })
                .collect();
            out.lines.push(Line::path(pts, 1.5 * s, color.with_alpha(0.85)));
        }
    }

    // Filler craft, only up to the cap with the economy's own craft counted first.
    if full && !theme.craft.is_empty() {
        let near = site.st.dock_radius * 4.0;
        let real = world
            .ships
            .values()
            .filter(|sh| {
                world.player.ship.as_ref() != Some(&sh.id)
                    && sh.docked_at.is_none()
                    && sh.position.distance(site.st.position) < near
            })
            .count() as u32;
        let free = calm.max_moving_craft.saturating_sub(real);
        let paint = theme
            .paint_faction(Some(site.st))
            .and_then(|f| f.parse::<FactionId>().ok())
            .unwrap_or_else(|| FactionId::from_static("faction.none"));
        let centre = g(site.st.position);
        for i in 0..free {
            // A wide slow circuit that stays clear of every module and sign.
            let Some(radius) = [2.4f32, 2.8, 3.2, 3.6].into_iter().find_map(|k| {
                let r = site.st.dock_radius * (k + 0.3 * i as f32);
                Site::circle(centre, r, super::SHIP_HOVER, 48)
                    .iter()
                    .all(|p| clear.ok(*p, 0.0, 8.0 * s))
                    .then_some(r)
            }) else {
                continue;
            };
            let seed = seed_of(&format!("{station}.filler.{i}"));
            let w = 0.03 + (seed % 5) as f32 * 0.004;
            let a = (seed % 628) as f32 / 100.0 + time * w;
            out.craft.push(Filler {
                hull: theme.craft[i as usize % theme.craft.len()].clone(),
                faction: paint.clone(),
                at: centre + G2::from_angle(a) * radius,
                heading: a + std::f32::consts::FRAC_PI_2,
            });
        }
    }

    out.particles = calm.max_particles - particles;
    out
}

/// A crowd's anchor from a happening: a part at this station by ID, or a kit anchor by
/// name.
fn anchor_point(world: &WorldState, site: &Site, name: &str) -> Option<(G2, f32)> {
    if let Some(p) = site.parts.iter().find(|p| p.id.as_str() == name) {
        return modules::module_point(world, p);
    }
    let a = render::kit::anchors(site.st.id.as_str(), &site.recipe)
        .into_iter()
        .find(|a| a.name == name)?;
    Some((
        polar(site.c, site.s, a.at.angle, a.at.distance),
        4.0 * site.s,
    ))
}

/// The paths residents walk for a spine.
fn walk_paths(world: &WorldState, site: &Site, spine: Spine) -> Vec<Vec<Vec3>> {
    let (deck_r, deck_z) = site.deck;
    match spine {
        // Along the docking ring, among the berths.
        Spine::Pier => match site.ring {
            Some((c, r, z)) => vec![Site::circle(c, r, z, 72)],
            None => vec![site.quay(QUAY_R, 48)],
        },
        // Round the hub's top deck.
        Spine::Ring => vec![Site::circle(site.c, deck_r * 0.62, deck_z, 40)],
        // Along the modules' front.
        Spine::Quay => vec![site.quay(QUAY_R, 48)],
        // Close about the centre, and out to each module and back.
        Spine::Spire => {
            let mut paths = vec![Site::circle(site.c, deck_r * 0.4, deck_z, 32)];
            paths.extend(
                site.parts
                    .iter()
                    .filter_map(|p| modules::module_point(world, p))
                    .map(|(at, _)| {
                        let deg = heading(at - site.c).to_degrees() + 8.0;
                        let far = at.distance(site.c) / site.s;
                        (0..16)
                            .map(|i| {
                                polar(site.c, site.s, deg, 9.0 + (far - 9.0) * i as f32 / 15.0)
                                    .extend(0.0)
                            })
                            .collect()
                    }),
            );
            paths
        }
    }
}

/// The crane: between the market's yard and the player's berth, swinging the player's
/// latest sale here from the berth to the yard over the ticks after it.
fn crane(
    world: &WorldState,
    history: &History,
    site: &Site,
    clear: &Clear,
    yard: G2,
    now: u64,
) -> Option<Crane> {
    let s = site.s;
    let ship = world.player.ship.as_ref()?;
    let berth = super::berth(site.st, ship).0;
    let height = CRANE_HEIGHT * s;
    let mast = [0.3f32, 0.4, 0.2, 0.5]
        .into_iter()
        .map(|k| yard.lerp(berth, k))
        .find(|p| clear.ok(p.extend(0.0), height, 0.5 * s))?;
    let reach_yard = mast.distance(yard).max(2.0 * s);
    let to_yard = heading(yard - mast);
    let mut out = Crane {
        at: mast.extend(0.0),
        yaw: to_yard,
        height,
        reach: reach_yard,
        load: None,
    };
    let Some(sale) = history.last(&HistoryQuery {
        kind: Some(FactKind::from_static("sell")),
        ship: Some(ship.clone()),
        station: Some(site.st.id.clone()),
        since: Some(Tick(now.saturating_sub(CARRY_TICKS))),
        ..HistoryQuery::default()
    }) else {
        return Some(out);
    };
    let Some(ware) = sale.subjects.ware.clone() else {
        return Some(out);
    };
    let k = now.saturating_sub(sale.tick.0) as f32 / CARRY_TICKS as f32;
    let to_berth = heading(berth - mast);
    let reach_berth = mast.distance(berth).max(2.0 * s);
    // Swing the short way round, from the berth to the yard.
    let mut turn = (to_yard - to_berth).rem_euclid(std::f32::consts::TAU);
    if turn > std::f32::consts::PI {
        turn -= std::f32::consts::TAU;
    }
    let deck = super::SHIP_HOVER;
    let carry = (height * 0.7).max(deck + 2.0 * s);
    let (yaw, reach, z) = if k < 0.15 {
        (
            to_berth,
            reach_berth,
            deck + (carry - deck) * smooth(k / 0.15),
        )
    } else if k < 0.8 {
        let m = smooth((k - 0.15) / 0.65);
        (
            to_berth + turn * m,
            reach_berth + (reach_yard - reach_berth) * m,
            carry,
        )
    } else {
        let m = smooth((k - 0.8) / 0.2);
        (to_yard, reach_yard, carry * (1.0 - m) + 2.0 * CRATE_SIZE * s * m)
    };
    out.yaw = yaw;
    out.reach = reach;
    let hang = (mast + G2::from_angle(yaw) * reach).extend(z.max(0.0));
    // Never drawn over a module or a sign: a load whose way crosses one is only seen
    // either side of it.
    if clear.ok(hang, CRATE_SIZE * s, 0.3 * s) {
        out.load = Some((ware, hang));
    }
    Some(out)
}

/// The colours a theme paints with at a station: its faction's, or the default.
pub fn palette_for(theme: &Theme, station: Option<&Station>) -> StationPalette {
    theme
        .paint_faction(station)
        .and_then(|f| ArtBible::bundled().faction(f))
        .map(StationPalette::from_faction)
        .unwrap_or_default()
}

/// A resident's coat and hat in a palette.
pub fn resident_colors(r: Resident, pal: &StationPalette) -> (Color, Color, Color) {
    let skins = [
        Color::hex(0xe8b48a),
        Color::hex(0xc98a5e),
        Color::hex(0x8d5a3b),
        Color::hex(0xf0c9a0),
    ];
    let (coat, hat) = match r {
        Resident::Dockhand => (pal.trim, pal.accent),
        Resident::Lamplighter => (pal.accent, pal.glow),
        Resident::Grower => (pal.leaf, pal.hull),
        Resident::Clerk => (pal.accent.lerp(pal.trim, 0.4), pal.trim),
        Resident::Porter => (pal.hull.lerp(pal.trim, 0.55), pal.accent),
        Resident::Scholar => (pal.trim.lerp(pal.glass, 0.3), pal.accent),
        Resident::Welder => (pal.trim.lerp(pal.accent, 0.5), pal.glow),
    };
    (coat, hat, skins[r as usize % skins.len()])
}

/// A ware's paint on its crates: what's inside, roughly.
pub fn ware_paint(ware: &WareId, pal: &StationPalette) -> Color {
    match ware.as_str() {
        "ware.ice" => Color::hex(0xbfe6ee),
        "ware.iron_ore" => Color::hex(0xb3693a),
        "ware.refined_plate" => Color::hex(0xc9a06a),
        "ware.glassite" => Color::hex(0x9fd8c4),
        "ware.rations" => Color::hex(0xe7b15a),
        "ware.ancients_fragment" => Color::hex(0xb9a2e8),
        other => pal.hull.lerp(pal.accent, (seed_of(other) % 100) as f32 / 140.0),
    }
}

/// The station's trading craft heading in to dock: where they are and their berth, so
/// the view can draw their approach.
pub fn approaches(world: &WorldState, station: &StationId) -> Vec<(G2, G2)> {
    let Some(st) = world.stations.get(station) else {
        return Vec::new();
    };
    world
        .ships
        .values()
        .filter(|sh| world.traders.contains_key(&sh.id) && sh.docked_at.is_none())
        .filter(|sh| {
            matches!(
                sh.orders.orders.get(sh.orders.current),
                Some(Order::Dock { station: s }) if s == station
            )
        })
        .filter(|sh| sh.position.distance(st.position) < st.dock_radius * 6.0)
        .map(|sh| (g(sh.position), super::berth(st, &sh.id).0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_read_what_they_know_and_ignore_the_rest() {
        let s = Shows::read([
            "banner_string:lamps",
            "crowd:part.wharf.office",
            "residents:+6",
            "fireworks:big",
            "residents:lots",
        ]);
        assert!(s.lamp_string);
        assert_eq!(s.crowds, ["part.wharf.office"]);
        assert_eq!(s.residents, 6);
    }

    #[test]
    fn piles_follow_stock() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        let world = galaxy_core::new_game(&dir, 1).unwrap().world;
        let mut line = world.markets.values().next().unwrap().lines.values().next().unwrap().clone();
        line.stock = 100;
        line.target = 100;
        assert_eq!(crates_for(&line), 4);
        line.stock = 0;
        assert_eq!(crates_for(&line), 0);
        line.stock = 1000;
        assert_eq!(crates_for(&line), 8);
        line.stock = 30;
        assert_eq!(crates_for(&line), 1);
    }

    #[test]
    fn walkers_pace_their_stretch_and_turn_back() {
        let run = vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)];
        assert_eq!(along(&run, 4.0).0, Vec3::new(4.0, 0.0, 0.0));
        let (p, facing) = along(&run, 14.0);
        assert!((p.x - 6.0).abs() < 1e-4, "{p}");
        assert!((facing.abs() - std::f32::consts::PI).abs() < 1e-4);
    }

    #[test]
    fn settings_round_trip_and_bad_files_fall_back() {
        let dir = std::env::temp_dir().join(format!("galaxy-view-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(SETTINGS_FILE);
        let _ = std::fs::remove_file(&path);
        assert_eq!(ViewSettings::load(&path), (ViewSettings::default(), None));
        let quiet = ViewSettings {
            life: LifeLevel::Quiet,
        };
        quiet.save(&path).unwrap();
        assert_eq!(ViewSettings::load(&path).0, quiet);
        std::fs::write(&path, "life = \"loud\"\n").unwrap();
        let (s, note) = ViewSettings::load(&path);
        assert_eq!(s, ViewSettings::default());
        assert!(note.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

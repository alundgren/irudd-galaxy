//! Effects on the generic `tests/play/effects` module, drawn only: the exhaust trails
//! while the ship pushes ahead, the beam lands on the rock's surface with sparks and
//! chips, docking puffs and kicks the camera, lamps wear halos, the camera widens with
//! speed and looks ahead, reduced motion keeps it still, a runaway recipe is capped, and
//! two views of the same session draw the very same particles. None of it touches the
//! world.
use galaxy_module::module::Module;
use galaxy_render::glam::{Vec2, Vec3};
use galaxy_render::{LineStyle, MAX_PARTICLES, ParticleShape, Scene};
use galaxy_sim::{Input, Session};
use galaxy_view::View;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

const SIZE: Vec2 = Vec2::new(1280.0, 720.0);

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/effects")
}

fn module() -> Module {
    Module::read(&folder()).unwrap()
}

fn key(tick: u64, name: &str, down: bool) -> Input {
    let mut payload = Map::new();
    payload.insert("down".into(), Value::Bool(down));
    Input {
        tick,
        source: "keyboard".into(),
        name: format!("effects.{name}"),
        payload,
    }
}

/// Steps the session once, with the view keeping its history first.
fn step(s: &mut Session, v: &mut View, inputs: &[Input]) {
    v.before_step(&s.world);
    s.step(inputs).unwrap();
}

fn ship_point(s: &Session, id: &str) -> Vec3 {
    let p = &s.world.component(id, "engine.position").unwrap().fields;
    let n = |k: &str| p[k].as_f64().unwrap() as f32;
    Vec3::new(n("x"), n("y"), n("height"))
}

#[test]
fn the_exhaust_sprays_only_while_pushing_and_stays_where_it_was_sprayed() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    for _ in 0..10 {
        step(&mut s, &mut v, &[]);
    }
    assert!(v.scene(&s.world, 0.5, SIZE, 0.0).particles.is_empty());
    let next = s.world.tick + 1;
    step(&mut s, &mut v, &[key(next, "thrust_ahead", true)]);
    for _ in 0..20 {
        step(&mut s, &mut v, &[]);
    }
    let scene = v.scene(&s.world, 0.5, SIZE, 0.0);
    assert!(scene.particles.len() > 10, "{}", scene.particles.len());
    // Flying +x, the trail is behind the ship, spread back along its path.
    let ship = v.pose(&s.world, "effects.ship", 0.5).unwrap();
    let xs: Vec<f32> = scene.particles.iter().map(|p| p.position.x).collect();
    assert!(xs.iter().all(|&x| x < ship.x - 1.0), "{xs:?} vs {}", ship.x);
    let spread =
        xs.iter().copied().fold(f32::MIN, f32::max) - xs.iter().copied().fold(f32::MAX, f32::min);
    assert!(
        spread > 3.0,
        "a trail, not a clump that rides along: {spread}"
    );
    // Let go: the trail dies away.
    let next = s.world.tick + 1;
    step(&mut s, &mut v, &[key(next, "thrust_ahead", false)]);
    for _ in 0..30 {
        step(&mut s, &mut v, &[]);
    }
    assert!(v.scene(&s.world, 0.5, SIZE, 0.0).particles.is_empty());
}

/// Mines the nearest rock and waits for the hold to land.
fn mining() -> (Session, View) {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    step(&mut s, &mut v, &[key(1, "order_mine", true)]);
    for _ in 0..600 {
        step(&mut s, &mut v, &[]);
        let beam = &s
            .world
            .component("effects.ship", "engine.beam")
            .unwrap()
            .fields;
        // An empty ID reads as null.
        if beam["target"].as_str().is_some_and(|t| !t.is_empty()) {
            for _ in 0..15 {
                step(&mut s, &mut v, &[]);
            }
            return (s, v);
        }
    }
    panic!("the mine order never held");
}

#[test]
fn the_beam_lands_on_the_rock_surface_with_sparks_and_chips() {
    let (s, mut v) = mining();
    let target = s
        .world
        .component("effects.ship", "engine.beam")
        .unwrap()
        .fields["target"]
        .as_str()
        .unwrap()
        .to_owned();
    let scene = v.scene(&s.world, 0.5, SIZE, 0.0);
    let beam = scene
        .lines
        .iter()
        .find(|l| l.style == LineStyle::Beam)
        .unwrap_or_else(|| {
            panic!(
                "no beam: {:?} {:?}",
                s.world.component("effects.ship", "engine.beam"),
                v.problems()
            )
        });
    let (from, to) = (beam.points[0], beam.points[1]);
    let rock = v.pose(&s.world, &target, 0.5).unwrap().point();
    let reach = v.reach(&s.world, &target).unwrap();
    let ship = v.pose(&s.world, "effects.ship", 0.5).unwrap().point();
    // It starts past the nose, and ends on the rock's surface: nearer the rock's middle
    // than its outline's reach, and well clear of its middle.
    assert!((from - ship).truncate().length() > 2.0, "{from} {ship}");
    let d = (to - rock).truncate().length();
    assert!(d < reach + 0.05 && d > reach * 0.3, "{d} of {reach}");
    // Halos at both ends, and sparks and chips flying from where it lands.
    assert!(
        scene
            .glows
            .iter()
            .any(|g| (g.position - to).length() < 1e-3)
    );
    let sparks = scene
        .particles
        .iter()
        .filter(|p| p.shape == ParticleShape::Spark);
    let chips = scene
        .particles
        .iter()
        .filter(|p| p.shape == ParticleShape::Chip);
    assert!(sparks.clone().count() > 3 && chips.count() > 0);
    assert!(sparks.clone().all(|p| p.additive));
    for p in sparks {
        assert!((p.position - to).length() < 3.0, "{} from {to}", p.position);
    }
}

#[test]
fn the_beam_follows_a_moving_ship_between_ticks() {
    let (s, mut v) = mining();
    let ends = |v: &mut View, alpha: f32| {
        let scene = v.scene(&s.world, alpha, SIZE, 0.0);
        let beam = scene
            .lines
            .iter()
            .find(|l| l.style == LineStyle::Beam)
            .unwrap();
        (
            beam.points[0],
            v.pose(&s.world, "effects.ship", alpha).unwrap(),
        )
    };
    for alpha in [0.0, 0.5, 1.0] {
        let (from, pose) = ends(&mut v, alpha);
        let nose = pose.point() + Vec3::new(pose.angle.cos() * 2.3, pose.angle.sin() * 2.3, 0.2);
        assert!((from - nose).length() < 1e-3, "{alpha}: {from} vs {nose}");
    }
}

fn dock() -> (Session, View, u64) {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    step(&mut s, &mut v, &[key(1, "order_dock", true)]);
    for _ in 0..900 {
        step(&mut s, &mut v, &[]);
        let burst = &s
            .world
            .component("effects.ship", "engine.burst")
            .unwrap()
            .fields;
        let tick = burst["tick"].as_u64().unwrap_or(0);
        if tick > 0 {
            return (s, v, tick);
        }
    }
    panic!("never docked");
}

#[test]
fn docking_puffs_and_kicks_the_camera_unless_motion_is_reduced() {
    let (mut s, mut v, tick) = dock();
    assert_eq!(s.world.tick, tick);
    let puffs = |scene: &Scene| {
        scene
            .particles
            .iter()
            .filter(|p| p.shape == ParticleShape::Puff && !p.additive)
            .count()
    };
    let scene = v.scene(&s.world, 0.5, SIZE, 0.0);
    assert_eq!(puffs(&scene), 18);
    let shaken = scene.camera;
    v.eye.still = true;
    let still = v.scene(&s.world, 0.5, SIZE, 0.0).camera;
    v.eye.still = false;
    let moved = (shaken.target - still.target).length();
    assert!(moved > 0.05, "the kick moved the camera: {moved}");
    assert!((shaken.eye - shaken.target - (still.eye - still.target)).length() < 1e-4);
    // Both die away.
    for _ in 0..30 {
        step(&mut s, &mut v, &[]);
    }
    let scene = v.scene(&s.world, 0.5, SIZE, 0.0);
    assert_eq!(puffs(&scene), 0);
    v.eye.still = true;
    let still = v.scene(&s.world, 0.5, SIZE, 0.0).camera;
    assert!((scene.camera.target - still.target).length() < 1e-4);
}

#[test]
fn the_camera_widens_with_speed_and_looks_ahead() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    step(&mut s, &mut v, &[key(1, "thrust_ahead", true)]);
    for _ in 0..60 {
        step(&mut s, &mut v, &[]);
    }
    let fast = v.scene(&s.world, 0.5, SIZE, 0.0).camera;
    v.eye.still = true;
    let plain = v.scene(&s.world, 0.5, SIZE, 0.0).camera;
    let ship = v.pose(&s.world, "effects.ship", 0.5).unwrap().point();
    assert!((plain.target - ship).length() < 1e-3);
    // Ahead (+x) of the ship, and showing more of the world.
    assert!(fast.target.x > ship.x + 1.0, "{} vs {}", fast.target, ship);
    let shown = |c: &galaxy_render::Camera| c.world_per_pixel(SIZE) * SIZE.y;
    assert!(
        shown(&fast) > shown(&plain) * 1.15,
        "{} vs {}",
        shown(&fast),
        shown(&plain)
    );
    // From one frame to the next it moves smoothly: no jump between ticks.
    v.eye.still = false;
    let mut last = None;
    for _ in 0..10 {
        step(&mut s, &mut v, &[]);
        for alpha in [0.25, 0.5, 0.75, 1.0] {
            let at = v.scene(&s.world, alpha, SIZE, 0.0).camera.target;
            if let Some(prev) = last {
                let d: Vec3 = at - prev;
                assert!(d.length() < 0.5, "jumped {}", d.length());
            }
            last = Some(at);
        }
    }
}

#[test]
fn lamps_wear_halos_their_looks_ask_for() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    step(&mut s, &mut v, &[]);
    let scene = v.scene(&s.world, 1.0, SIZE, 0.0);
    // The beacon's lamp sits 2.8 up; its look halo is 1.4 at 3 times the lamp's size.
    let beacon = ship_point(&s, "effects.beacon");
    let halo = scene
        .glows
        .iter()
        .find(|g| (g.position.truncate() - beacon.truncate()).length() < 0.2)
        .expect("a halo on the beacon's lamp");
    assert!((halo.position.z - 2.8).abs() < 0.3, "{}", halo.position);
    assert!(halo.radius > 1.0);
    // Rocks have no self-lit paint and no halo.
    let rock = ship_point(&s, "effects.rock1");
    assert!(
        !scene
            .glows
            .iter()
            .any(|g| (g.position.truncate() - rock.truncate()).length() < 1.0)
    );
}

#[test]
fn two_views_of_one_session_draw_the_same_particles_and_the_world_never_changes() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut a = View::detached(s.modules(), 1);
    let mut b = View::detached(s.modules(), 1);
    let mut plain = Session::new(vec![module()], 1).unwrap();
    let inputs = [key(1, "thrust_ahead", true), key(1, "turn_left", true)];
    for k in 0..90 {
        let now: &[Input] = if k == 0 { &inputs } else { &[] };
        a.before_step(&s.world);
        b.before_step(&s.world);
        s.step(now).unwrap();
        plain.step(now).unwrap();
        // One view draws every tick, the other only now and then.
        let hash = s.world.hash();
        a.scene(&s.world, 0.3, SIZE, 0.0);
        assert_eq!(s.world.hash(), hash, "drawing never writes the world");
        if k % 17 == 0 {
            let pa = a.scene(&s.world, 0.7, SIZE, 0.0).particles;
            let pb = b.scene(&s.world, 0.7, SIZE, 0.0).particles;
            assert_eq!(pa, pb, "tick {}", s.world.tick);
        }
    }
    assert_eq!(s.world.hash(), plain.world.hash());
}

/// A copy of the effects module in `dir`, to change.
fn copied(dir: &Path) -> PathBuf {
    let copy = dir.join("effects");
    for sub in ["", "models", "scripts"] {
        std::fs::create_dir_all(copy.join(sub)).unwrap();
        for e in std::fs::read_dir(folder().join(sub)).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_file() {
                std::fs::copy(e.path(), copy.join(sub).join(e.file_name())).unwrap();
            }
        }
    }
    copy
}

#[test]
fn a_runaway_recipe_is_capped_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let copy = copied(dir.path());
    let mut style = std::fs::read_to_string(copy.join("style.toml")).unwrap();
    style
        .push_str("\n[particles.flood]\nrate = 400.0\nlife = 4.0\nlife_jitter = 0.0\ndrag = 0.0\n");
    std::fs::write(copy.join("style.toml"), style).unwrap();
    let mut start = std::fs::read_to_string(copy.join("start.toml")).unwrap();
    for rock in ["rock1", "rock2", "rock3"] {
        start = start.replace(
            &format!("[entities.{rock}]\n"),
            &format!("[entities.{rock}]\n\"engine.emitter\" = {{ recipe = \"effects.flood\" }}\n"),
        );
    }
    std::fs::write(copy.join("start.toml"), start).unwrap();
    let mut s = Session::new(vec![Module::read(&copy).unwrap()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    for _ in 0..130 {
        step(&mut s, &mut v, &[]);
    }
    let scene = v.scene(&s.world, 0.5, SIZE, 0.0);
    assert_eq!(scene.particles.len(), MAX_PARTICLES);
    assert!(
        v.problems()
            .iter()
            .any(|p| p.contains("more particles at once")),
        "{:?}",
        v.problems()
    );
}

#[test]
fn a_missing_recipe_or_beam_look_is_a_problem() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    let ship = s.world.entities.get_mut("effects.ship").unwrap();
    ship.get_mut("engine.emitter")
        .unwrap()
        .fields
        .insert("recipe".into(), "effects.nope".into());
    step(&mut s, &mut v, &[]);
    v.scene(&s.world, 0.5, SIZE, 0.0);
    assert!(
        v.problems()
            .iter()
            .any(|p| p.contains("particle recipe `effects.nope`")),
        "{:?}",
        v.problems()
    );
}

#[test]
fn many_runaway_emitters_stop_making_particles_at_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    let copy = copied(dir.path());
    let mut style = std::fs::read_to_string(copy.join("style.toml")).unwrap();
    style
        .push_str("\n[particles.flood]\nrate = 400.0\nlife = 4.0\nlife_jitter = 0.0\ndrag = 0.0\n");
    std::fs::write(copy.join("style.toml"), style).unwrap();
    // Each of these keeps 1600 particles alive: 200 of them would make 320,000.
    let mut start = std::fs::read_to_string(copy.join("start.toml")).unwrap();
    for n in 0..200 {
        start.push_str(&format!(
            "\n[entities.flood{n}]\n\"engine.position\" = {{ x = {}.0, y = 40.0 }}\n\
             \"engine.emitter\" = {{ recipe = \"effects.flood\" }}\n",
            n % 20
        ));
    }
    std::fs::write(copy.join("start.toml"), start).unwrap();
    let mut s = Session::new(vec![Module::read(&copy).unwrap()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    for _ in 0..130 {
        step(&mut s, &mut v, &[]);
    }
    let scene = v.scene(&s.world, 0.5, SIZE, 0.0);
    assert_eq!(scene.particles.len(), MAX_PARTICLES);
    // The scene never held more than one past the cap: the rest were never made.
    assert!(
        scene.particles.capacity() <= MAX_PARTICLES + 1,
        "{}",
        scene.particles.capacity()
    );
    assert!(
        v.problems()
            .iter()
            .any(|p| p.contains("more particles at once")),
        "{:?}",
        v.problems()
    );
}

#[test]
fn a_film_of_a_recording_keeps_reduced_motion_from_its_start() {
    use galaxy_sim::recording::{BuildStamp, Replay};
    use galaxy_view::film::{self, Shoot};
    let mut s = Session::new(vec![module()], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    v.eye.still = true;
    // Where the player's camera looked on each tick, drawn at the tick itself.
    let mut seen = std::collections::BTreeMap::new();
    for _ in 0..75 {
        let next = s.world.tick + 1;
        let held = [key(next, "thrust_ahead", true)];
        step(&mut s, &mut v, if next == 1 { &held } else { &[] });
        let camera = v.scene(&s.world, 0.0, SIZE, 0.0).camera;
        seen.insert(s.world.tick, camera.target);
        if s.world.tick == 60 {
            let moment = v.moment(0.0, 0.0, (1280, 720), 1.0).unwrap();
            assert!(moment.still);
            s.note("here", Some(serde_json::to_value(&moment).unwrap()), None);
        }
    }
    let rec = s.recording().unwrap();
    let moment: galaxy_view::Moment = rec
        .events
        .iter()
        .find_map(|e| match e {
            galaxy_sim::recording::RecEvent::Note {
                camera: Some(c), ..
            } => Some(serde_json::from_value(c.clone()).unwrap()),
            _ => None,
        })
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    let mut view = View::detached(replay.session.modules(), 1);
    let shoot = Shoot {
        fps: 30,
        from: 50,
        to: 70,
        width: 1280,
        height: 720,
        scale: 1.0,
        out: dir.path().join("film"),
        zooms: film::recorded_zooms(&rec),
        source: serde_json::json!({}),
        exact_note: Some((60, moment)),
    };
    let manifest = film::shoot(&mut replay, &mut view, None, &shoot).unwrap();
    assert!(manifest.frames.len() > 15);
    for frame in &manifest.frames {
        assert_eq!(frame.alpha, 0.0);
        let film = Vec3::from(frame.camera.target);
        let player = seen[&frame.tick];
        assert!(
            (film - player).length() < 1e-3,
            "tick {}: the film looks at {film}, the player at {player}",
            frame.tick
        );
    }
}

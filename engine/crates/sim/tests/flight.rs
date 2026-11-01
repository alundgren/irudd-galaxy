//! Flight, orders and collision on the `drift` test module (`engine/tests/play/drift`),
//! headless.

use std::path::{Path, PathBuf};

use galaxy_api::engine::{ARRIVED, BODY, MODEL, MOTION, POSITION, SCRIPT_FAILED, STEER};
use galaxy_module::module::Module;
use galaxy_sim::collide::outline;
use galaxy_sim::shape::{Shapes, overlap};
use galaxy_sim::{Input, Session, World};
use serde_json::{Map, Value};

fn drift_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/drift")
}

fn drift() -> Session {
    Session::new(vec![Module::read(&drift_dir()).unwrap()], 1).unwrap()
}

fn key(event: &str, down: bool) -> Input {
    let mut payload = Map::new();
    payload.insert("down".into(), Value::Bool(down));
    Input {
        tick: 0,
        source: "keyboard".into(),
        name: format!("drift.{event}"),
        payload,
    }
}

fn number(world: &World, entity: &str, component: &str, field: &str) -> f64 {
    world.component(entity, component).unwrap().fields[field]
        .as_f64()
        .unwrap()
}

fn at(world: &World, entity: &str) -> (f64, f64) {
    (
        number(world, entity, POSITION, "x"),
        number(world, entity, POSITION, "y"),
    )
}

/// Runs `ticks` ticks with `inputs` on the first, checking after each that no two solid
/// things overlap more than a hair.
fn run(session: &mut Session, inputs: &[Input], ticks: u32) {
    let shapes = Shapes::of(session.modules());
    for n in 0..ticks {
        session.step(if n == 0 { inputs } else { &[] }).unwrap();
        assert_no_overlaps(&session.world, &shapes);
    }
    let failures: Vec<_> = session
        .world
        .facts
        .iter()
        .filter(|f| f.kind == SCRIPT_FAILED)
        .collect();
    assert!(failures.is_empty(), "{failures:?}");
}

fn assert_no_overlaps(world: &World, shapes: &Shapes) {
    let bodies: Vec<&String> = world
        .entities
        .iter()
        .filter(|(_, c)| c.contains_key(BODY))
        .map(|(id, _)| id)
        .collect();
    for (i, a) in bodies.iter().enumerate() {
        for b in &bodies[i + 1..] {
            let (oa, ob) = (
                outline(world, shapes, a).unwrap(),
                outline(world, shapes, b).unwrap(),
            );
            if let Some((depth, _)) = overlap(&oa, at(world, a), &ob, at(world, b)) {
                assert!(
                    depth < 0.05,
                    "tick {}: {a} and {b} overlap by {depth}",
                    world.tick
                );
            }
        }
    }
}

fn arrivals(world: &World) -> Vec<(String, String)> {
    world
        .facts
        .iter()
        .filter(|f| f.kind == ARRIVED)
        .map(|f| {
            (
                f.payload["mode"].as_str().unwrap().to_owned(),
                f.payload["target"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn keys_fly_the_ship_and_it_turns_half_a_circle_a_second() {
    let mut s = drift();
    run(&mut s, &[key("thrust_ahead", true)], 31);
    let w = &s.world;
    assert!(
        (number(w, "drift.ship", MOTION, "vx") - 14.0).abs() < 1e-6,
        "top speed"
    );
    assert!(
        number(w, "drift.ship", MOTION, "vy").abs() < 1e-9,
        "straight along the nose"
    );
    // Inputs are handled before the tick's flight, so the helm turns on the tick the key
    // goes down and stops on the tick it goes up: fifteen ticks.
    run(
        &mut s,
        &[key("thrust_ahead", false), key("turn_left", true)],
        15,
    );
    run(&mut s, &[key("turn_left", false)], 1);
    let angle = number(&s.world, "drift.ship", POSITION, "angle");
    assert!(
        (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "{angle}"
    );
    // Let go: it coasts to a stop within the stop time.
    run(&mut s, &[], 31);
    assert_eq!(number(&s.world, "drift.ship", MOTION, "vx"), 0.0);
    assert_eq!(number(&s.world, "drift.ship", MOTION, "vy"), 0.0);
}

#[test]
fn it_docks_on_the_berth_facing_its_way() {
    let mut s = drift();
    run(&mut s, &[key("order_dock", true)], 600);
    let (x, y) = at(&s.world, "drift.ship");
    let (bx, by) = at(&s.world, "drift.berth");
    assert!((x - bx).abs() < 0.06 && (y - by).abs() < 0.06, "{x} {y}");
    let angle = number(&s.world, "drift.ship", POSITION, "angle");
    assert!(
        (angle - std::f64::consts::FRAC_PI_2).abs() < 0.03,
        "{angle}"
    );
    assert_eq!(arrivals(&s.world), [("dock".into(), "drift.berth".into())]);
    // W flies straight off the dock: the order is done, the helm has the ship.
    run(&mut s, &[key("thrust_ahead", true)], 20);
    let (x2, y2) = at(&s.world, "drift.ship");
    assert!((x2 - x).abs() < 1e-6 && y2 > y + 2.0, "{x2} {y2}");
}

/// Where a thing's model is drawn on the sim plane: its vertices turned, scaled, morphed
/// and placed like the drawing.
fn footprint(world: &World, entity: &str) -> Vec<(f64, f64)> {
    let model = &world.component(entity, MODEL).unwrap().fields;
    let id = model["model"].as_str().unwrap();
    let name = id.strip_prefix("drift.").unwrap();
    let path = drift_dir().join(format!("models/{name}.gltf"));
    let mesh = galaxy_mesh::gltf::read(&std::fs::read_to_string(path).unwrap()).unwrap();
    let scale = model["scale"].as_f64().unwrap();
    let morph = model["morph"].as_f64().unwrap();
    let angle = number(world, entity, POSITION, "angle");
    let (x, y) = at(world, entity);
    let (c, s) = (angle.cos(), angle.sin());
    mesh.vertices
        .iter()
        .map(|v| {
            let mix = |a: f32, b: f32| f64::from(a) + (f64::from(b) - f64::from(a)) * morph;
            let px = mix(v.position[0], v.morph_position[0]) * scale;
            let py = mix(v.position[1], v.morph_position[1]) * scale;
            (x + px * c - py * s, y + px * s + py * c)
        })
        .collect()
}

/// The true distance between two drawn footprints (their convex outlines): the widest
/// gap along any direction, negative when they overlap.
fn drawn_gap(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    (0..3600)
        .map(|i| {
            let t = std::f64::consts::TAU * f64::from(i) / 3600.0;
            let (ux, uy) = (t.cos(), t.sin());
            let far_a = a
                .iter()
                .map(|p| p.0 * ux + p.1 * uy)
                .fold(f64::MIN, f64::max);
            let near_b = b
                .iter()
                .map(|p| p.0 * ux + p.1 * uy)
                .fold(f64::MAX, f64::min);
            near_b - far_a
        })
        .fold(f64::MIN, f64::max)
}

#[test]
fn mining_holds_at_the_rock_s_drawn_surface_from_any_side() {
    for side in 0..8 {
        let mut s = drift();
        // Start nine units off rock3, on side `side` of eight.
        let a = std::f64::consts::TAU * f64::from(side) / 8.0 + 0.2;
        let (rx, ry) = at(&s.world, "drift.rock3");
        let ship = s.world.entities.get_mut("drift.ship").unwrap();
        let pos = &mut ship.get_mut(POSITION).unwrap().fields;
        pos.insert("x".into(), Value::from(rx + 9.0 * a.cos()));
        pos.insert("y".into(), Value::from(ry + 9.0 * a.sin()));
        let order = &mut ship.get_mut(STEER).unwrap().fields;
        order.insert("mode".into(), Value::from("hold"));
        order.insert("target".into(), Value::from("drift.rock3"));
        run(&mut s, &[], 600);
        assert_eq!(
            arrivals(&s.world),
            [("hold".into(), "drift.rock3".into())],
            "side {side}"
        );
        let g = drawn_gap(
            &footprint(&s.world, "drift.ship"),
            &footprint(&s.world, "drift.rock3"),
        );
        assert!(
            (-0.02..0.1).contains(&g),
            "side {side}: the drawn gap between ship and rock is {g}"
        );
        // Facing the rock.
        let (sx, sy) = at(&s.world, "drift.ship");
        let (rx2, ry2) = at(&s.world, "drift.rock3");
        let toward = libm::atan2(ry2 - sy, rx2 - sx);
        let angle = number(&s.world, "drift.ship", POSITION, "angle");
        let off = galaxy_sim::flight::wrap(angle - toward);
        assert!(off.abs() < 0.05, "side {side}: {angle} vs {toward}");
        // Holding pushed nothing.
        let moved = (rx2 - rx).abs() + (ry2 - ry).abs();
        assert!(moved < 0.01, "side {side}: the rock moved {moved}");
    }
}

#[test]
fn a_hold_reaches_a_big_round_body_without_shoving_it() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("big");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            "id = \"big\"\nversion = \"0.1.0\"\nmin_engine_version = \"{}\"\n\
             writes = [\"engine.position\", \"engine.motion\", \"engine.flight\", \
             \"engine.steer\", \"engine.body\"]\n",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    // The rock's bearing from the ship lies halfway between two sampled directions.
    std::fs::write(
        dir.join("start.toml"),
        "[entities.ship]\n\
         \"engine.position\" = {}\n\"engine.motion\" = {}\n\"engine.flight\" = {}\n\
         \"engine.body\" = { radius = 1.0 }\n\
         \"engine.steer\" = { mode = \"hold\", target = \"big.rock\" }\n\
         [entities.rock]\n\
         \"engine.position\" = { x = 39.808, y = 3.921 }\n\"engine.motion\" = {}\n\
         \"engine.body\" = { radius = 12.0, mass = 8.0 }\n",
    )
    .unwrap();
    let mut s = Session::new(vec![Module::read(&dir).unwrap()], 1).unwrap();
    for _ in 0..900 {
        s.step(&[]).unwrap();
    }
    assert_eq!(arrivals(&s.world), [("hold".into(), "big.rock".into())]);
    let (x, y) = at(&s.world, "big.rock");
    assert!(
        (x - 39.808).abs() < 1e-9 && (y - 3.921).abs() < 1e-9,
        "shoved to {x} {y}"
    );
    let (sx, sy) = at(&s.world, "big.ship");
    let apart = ((x - sx).powi(2) + (y - sy).powi(2)).sqrt();
    assert!(
        (apart - 13.0).abs() < 0.1,
        "{apart} apart, for radii 1 and 12"
    );
}

#[test]
fn it_follows_the_route_to_its_end() {
    let mut s = drift();
    run(&mut s, &[key("order_route", true)], 900);
    let (x, y) = at(&s.world, "drift.ship");
    assert!((x - 30.0).abs() < 0.06 && (y - 6.0).abs() < 0.06, "{x} {y}");
    assert_eq!(arrivals(&s.world), [("follow".into(), "drift.w3".into())]);
}

#[test]
fn ramming_a_rock_pushes_it_and_the_fixed_hub_stops_the_ship_without_trapping_it() {
    let mut s = drift();
    // rock4 sits at (-20, 2): turn round to face -x and thrust into it.
    run(&mut s, &[key("turn_left", true)], 30);
    let start = at(&s.world, "drift.rock4");
    run(
        &mut s,
        &[key("turn_left", false), key("thrust_ahead", true)],
        150,
    );
    let pushed = at(&s.world, "drift.rock4");
    let shoved = libm::sqrt((pushed.0 - start.0).powi(2) + (pushed.1 - start.1).powi(2));
    assert!(shoved > 1.0, "rock4 was pushed: {start:?} to {pushed:?}");
    // Now face the hub at (-24, -18) and fly into it for a while.
    let (sx, sy) = at(&s.world, "drift.ship");
    let toward = libm::atan2(-18.0 - sy, -24.0 - sx);
    let mut s2 = s;
    let mut turns = 0;
    loop {
        let angle = number(&s2.world, "drift.ship", POSITION, "angle");
        let off = galaxy_sim::flight::wrap(toward - angle);
        if off.abs() < 0.12 || turns > 60 {
            break;
        }
        let dir = if off > 0.0 { "turn_left" } else { "turn_right" };
        run(&mut s2, &[key(dir, true)], 1);
        run(&mut s2, &[key(dir, false)], 1);
        turns += 1;
    }
    run(&mut s2, &[], 200);
    let hub = at(&s2.world, "drift.hub");
    assert_eq!(hub, (-24.0, -18.0), "the hub is fixed");
    // Back away: it isn't stuck.
    let before = at(&s2.world, "drift.ship");
    run(
        &mut s2,
        &[key("thrust_ahead", false), key("thrust_back", true)],
        30,
    );
    let after = at(&s2.world, "drift.ship");
    let moved = libm::sqrt((after.0 - before.0).powi(2) + (after.1 - before.1).powi(2));
    assert!(moved > 2.0, "backed off {moved}");
}

/// The world hash after the flight session below. The same on every OS: flight and
/// collision use plain arithmetic and `libm` only. Change it only on purpose.
const GOLDEN: &str = "f99624b4fdd9c578ca123c721594b4314493ff29acb9137494a45ea2a78bf38d";

#[test]
fn the_same_inputs_give_the_same_world() {
    let play = || {
        let mut s = drift();
        run(
            &mut s,
            &[key("thrust_ahead", true), key("turn_left", true)],
            90,
        );
        run(&mut s, &[key("turn_left", false)], 120);
        run(&mut s, &[key("order_mine", true)], 300);
        s.world.hash()
    };
    let hash = play();
    assert_eq!(hash, play());
    assert_eq!(
        hash, GOLDEN,
        "the flight session's world changed; if on purpose, update GOLDEN"
    );
}

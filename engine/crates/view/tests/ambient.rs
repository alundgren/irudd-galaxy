//! A world that moves with nothing pressed, and labels that yield, on the generic
//! `tests/play/ambient` module: a rock's steady spin, a station's spinning ring and
//! pulsing lamps (drawn only), star layers, a hold that keeps contact as its rock is cut,
//! and a trader's name label that isn't drawn where it would cover the status panel.
use galaxy_api::engine::{MODEL, POSITION, STEER};
use galaxy_module::module::Module;
use galaxy_render::Rect;
use galaxy_render::glam::Vec2;
use galaxy_sim::{Input, Session, World};
use galaxy_view::View;
use galaxy_view::screens::Boxed;
use serde_json::{Map, Value};
use std::path::Path;

fn module() -> Module {
    Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/ambient")).unwrap()
}

const SIZE: Vec2 = Vec2::new(1280.0, 720.0);

fn find<'a>(boxes: &'a [Boxed], id: &str) -> Option<&'a Boxed> {
    fn walk<'a>(b: &'a Boxed, id: &str) -> Option<&'a Boxed> {
        if b.widget.id.as_deref() == Some(id) {
            return Some(b);
        }
        b.children.iter().find_map(|c| walk(c, id))
    }
    boxes.iter().find_map(|b| walk(b, id))
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn number(world: &World, entity: &str, component: &str, field: &str) -> f64 {
    world.component(entity, component).unwrap().fields[field]
        .as_f64()
        .unwrap()
}

#[test]
fn a_yielding_label_is_hidden_where_it_would_cover_the_panel_or_leave_the_window() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    // Zoomed fully in, as the request asks.
    v.zoom(&s.world, 100.0);
    let (mut covered, mut shown, mut offscreen) = (0, 0, 0);
    for _ in 0..900 {
        v.before_step(&s.world);
        s.step(&[]).unwrap();
        v.scene(&s.world, 0.5, SIZE, 0.0);
        let label = find(&v.screens.boxes, "trader-label").unwrap();
        let status = find(&v.screens.boxes, "status").unwrap().rect;
        // The window less the 8 px margin, to the half pixel the engine allows.
        let inside = label.rect.x >= 7.5
            && label.rect.y >= 7.5
            && label.rect.x + label.rect.w <= SIZE.x - 7.5
            && label.rect.y + label.rect.h <= SIZE.y - 7.5;
        let over = overlaps(label.rect, status);
        if label.hidden {
            if over {
                covered += 1;
            } else if !inside || !v.screens.anchors.contains_key("ambient.trader_label#label") {
                offscreen += 1;
            }
        } else {
            assert!(
                !over && inside,
                "tick {}: shown at {:?}",
                s.world.tick,
                label.rect
            );
            shown += 1;
        }
        // Nothing reported: a hidden label overlaps nothing and isn't clamped.
        assert!(
            v.problems().is_empty(),
            "tick {}: {:?}",
            s.world.tick,
            v.problems()
        );
    }
    assert!(
        covered > 5,
        "the trader passed under the panel: {covered} ticks"
    );
    assert!(shown > 5, "and was named clear of it: {shown} ticks");
    assert!(
        offscreen > 5,
        "and flew off screen unnamed: {offscreen} ticks"
    );
}

#[test]
fn a_yielding_label_never_hides_behind_a_click_or_a_bot() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    v.zoom(&s.world, 100.0);
    for _ in 0..900 {
        v.before_step(&s.world);
        s.step(&[]).unwrap();
        v.scene(&s.world, 0.5, SIZE, 0.0);
        let label = find(&v.screens.boxes, "trader-label").unwrap();
        if label.hidden {
            // A hidden label paints nothing and isn't under the pointer.
            let centre = label.rect.center();
            let hit = v.screens.click(centre, s.world.tick);
            assert!(hit.is_none());
            let scene = v.scene(&s.world, 0.5, SIZE, 0.0);
            let drawn = scene
                .overlay
                .widgets
                .iter()
                .any(|l| l.texts.iter().any(|t| t.text == "Trader"));
            assert!(!drawn, "tick {}", s.world.tick);
            return;
        }
    }
    panic!("the label never yielded");
}

#[test]
fn the_ring_turns_and_the_lamps_pulse_on_screen_only() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    let mut rings = Vec::new();
    let mut glows = Vec::new();
    for _ in 0..90 {
        v.before_step(&s.world);
        s.step(&[]).unwrap();
        let hash = s.world.hash();
        let scene = v.scene(&s.world, 0.5, SIZE, 0.0);
        assert_eq!(s.world.hash(), hash, "drawing changes nothing");
        // The hub is drawn as its still parts, its ring and its lamps: three instances,
        // one of them glowing less than full while the lamps dim.
        let hub: Vec<_> = scene
            .meshes
            .iter()
            .filter(|m| {
                (m.transform.w_axis.x + 22.0).abs() < 1e-3
                    && (m.transform.w_axis.y + 12.0).abs() < 1e-3
            })
            .collect();
        assert_eq!(hub.len(), 3, "tick {}", s.world.tick);
        let still = hub[0].transform;
        let ring = hub
            .iter()
            .find(|m| m.transform != still)
            .expect("the ring turned");
        rings.push(ring.transform);
        glows.push(hub.iter().map(|m| m.glow).fold(1.0, f32::min));
    }
    // 8 degrees a second: each tick turns the ring's frame a little further, steadily.
    let angle = |m: &galaxy_render::glam::Mat4| m.x_axis.y.atan2(m.x_axis.x);
    for w in rings.windows(2) {
        let d = angle(&w[1]) - angle(&w[0]);
        assert!((d - (8f32 / 30.0).to_radians()).abs() < 1e-4, "{d}");
    }
    // A two-second pulse down to a tenth: dim halfway, bright at each end.
    let dim = glows.iter().copied().fold(1.0, f32::min);
    let bright = glows.iter().copied().fold(0.0, f32::max);
    assert!(dim < 0.15 && bright > 0.95, "{dim} {bright}");
    // A fresh view of the same world draws the same frame: time comes from the world.
    let mut again = View::detached(s.modules(), 1);
    let a = v.scene(&s.world, 0.25, SIZE, 0.0);
    let b = again.scene(&s.world, 0.25, SIZE, 7.0);
    assert_eq!(a.meshes.len(), b.meshes.len());
    for (x, y) in a.meshes.iter().zip(&b.meshes).filter(|(x, _)| x.glow < 1.0) {
        assert_eq!((x.transform, x.glow), (y.transform, y.glow));
    }
}

#[test]
fn the_rock_turns_steadily_and_the_sky_has_three_layers() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let v = View::detached(s.modules(), 1);
    assert_eq!(v.assets.sky.star_layers, [0.2, 0.6, 1.4]);
    let mut last = number(&s.world, "ambient.drifter", POSITION, "angle");
    for _ in 0..60 {
        s.step(&[]).unwrap();
        let now = number(&s.world, "ambient.drifter", POSITION, "angle");
        let d = galaxy_sim::flight::wrap(now - last);
        assert!((d - (12f64 / 30.0).to_radians()).abs() < 1e-12, "{d}");
        last = now;
    }
}

#[test]
fn the_hold_stays_on_the_rock_through_six_cuts() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut payload = Map::new();
    payload.insert("down".into(), Value::Bool(true));
    let mine = Input {
        tick: 1,
        source: "keyboard".into(),
        name: "ambient.order_mine".into(),
        payload,
        via: None,
    };
    s.step(&[mine]).unwrap();
    let shapes = galaxy_sim::shape::Shapes::of(s.modules());
    let gap = |w: &World| {
        let ship = galaxy_sim::collide::outline(w, &shapes, "ambient.ship").unwrap();
        let ore = galaxy_sim::collide::outline(w, &shapes, "ambient.ore").unwrap();
        let at = |e: &str| (number(w, e, POSITION, "x"), number(w, e, POSITION, "y"));
        let ((ax, ay), (bx, by)) = (at("ambient.ship"), at("ambient.ore"));
        let d = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
        let toward = (by - ay).atan2(bx - ax);
        d - galaxy_sim::shape::touching(&ship, &ore, toward)
    };
    let mut arrived = None;
    let mut widest: f64 = 0.0;
    for _ in 0..450 {
        s.step(&[]).unwrap();
        let steer = &s.world.component("ambient.ship", STEER).unwrap().fields;
        if arrived.is_none() && steer["done"] == true {
            arrived = Some(s.world.tick);
        }
        if arrived.is_some() {
            widest = widest.max(gap(&s.world));
        }
    }
    assert!(arrived.is_some());
    let morph = number(&s.world, "ambient.ore", MODEL, "morph");
    assert!((morph - 1.0).abs() < 1e-9, "six cuts: {morph}");
    assert!(widest < 0.05, "the gap opened to {widest}");
    let steer = &s.world.component("ambient.ship", STEER).unwrap().fields;
    assert_eq!(
        (steer["mode"].as_str(), steer["done"].as_bool()),
        (Some("hold"), Some(true))
    );
}

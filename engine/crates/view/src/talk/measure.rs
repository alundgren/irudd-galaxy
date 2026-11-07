use crate::View;
use galaxy_module::module::Module;
use galaxy_render::glam::Vec2;
use galaxy_sim::{Session, scenario::InputsFile};
use serde_json::{Value, json};
use std::{path::Path, time::Instant};

fn summary(mut values: Vec<f64>) -> Value {
    values.sort_by(f64::total_cmp);
    json!({"median":values[values.len()/2], "p95":values[values.len()*95/100], "max":values.last().unwrap()})
}

#[test]
#[ignore = "operator-selected headless route and runtime measurement"]
fn talk_cost_probe() {
    let module = Module::read(Path::new(&std::env::var("GALAXY_FOCUS_MODULE").unwrap())).unwrap();
    let route =
        InputsFile::read(Path::new(&std::env::var("GALAXY_FOCUS_INPUTS").unwrap())).unwrap();
    assert!(
        route.pointers.is_empty()
            && route.keys.is_empty()
            && route.resizes.is_empty()
            && route.zooms.is_empty()
            && route.hot_loads.is_empty()
    );
    let mut session = Session::new(vec![module], 1).unwrap();
    let mut view = View::detached(session.modules(), 1);
    view.focus_live = false;
    let size = Vec2::new(1280.0, 720.0);
    let by_tick = route.by_tick();
    let mut before = None;
    let before_tick: u64 = std::env::var("GALAXY_FOCUS_BEFORE")
        .unwrap()
        .parse()
        .unwrap();
    let end_tick: u64 = std::env::var("GALAXY_FOCUS_TICK").unwrap().parse().unwrap();
    assert!(before_tick > 0 && before_tick < end_tick);
    let mut recorded = Vec::new();
    for tick in 1..=end_tick {
        view.before_step(&session.world);
        let mut inputs = by_tick.get(&tick).cloned().unwrap_or_default();
        for click in route.clicks.iter().filter(|c| c.tick == tick) {
            view.scene(&session.world, 1.0, size, tick as f32 / 30.0);
            inputs.push(
                galaxy_sim::travel::arrival(&session.world, true)
                    .map(Ok)
                    .unwrap_or_else(|| view.bot_talk_click(&session.world, click))
                    .unwrap(),
            );
        }
        if !inputs
            .iter()
            .any(|i| galaxy_sim::travel::is_landing(&i.name))
            && let Some(arrived) = galaxy_sim::travel::arrival(&session.world, false)
        {
            inputs.push(arrived);
        }
        recorded.extend(inputs.iter().cloned());
        session.step(&inputs).unwrap();
        view.scene(&session.world, 1.0, size, tick as f32 / 30.0);
        if tick == before_tick {
            before = Some(session.world.clone());
        }
    }
    if let Ok(path) = std::env::var("GALAXY_FOCUS_INPUT_EXPORT") {
        #[derive(serde::Serialize)]
        struct Export {
            input: Vec<galaxy_sim::Input>,
        }
        std::fs::write(path, toml::to_string(&Export { input: recorded }).unwrap()).unwrap();
    }
    let after = session.world.clone();
    for (label, world, focus) in [
        ("before_talk", before.as_ref().unwrap(), true),
        ("talk", &after, true),
        ("talk_without_focus_check", &after, false),
    ] {
        let mut view = View::detached(session.modules(), 1);
        view.focus_live = focus;
        view.before_step(world);
        for _ in 0..5 {
            view.scene(world, 1.0, size, world.tick as f32 / 30.0);
        }
        let calls = view.screens.calls();
        let evaluations = view.focus_cache.evaluations();
        let mut times = vec![];
        for _ in 0..600 {
            let t = Instant::now();
            view.scene(world, 1.0, size, world.tick as f32 / 30.0);
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let on = view.scene(world, 1.0, size, world.tick as f32 / 30.0);
        view.focus_live = false;
        let off = view.scene(world, 1.0, size, world.tick as f32 / 30.0);
        assert_eq!(on.overlay, off.overlay);
        println!(
            "{}",
            json!({"case":label,"scene_ms":summary(times),"frames":600,
            "luau_rebuilds":view.screens.calls()-calls,"talk_widgets":crate::talk::widgets(world,&view.screens.style).len(),
            "pixel_evaluations":view.focus_cache.evaluations()-evaluations,
            "world_hash":world.hash()})
        );
    }
    let mut view = View::detached(session.modules(), 1);
    view.focus_live = false;
    view.before_step(&after);
    let scene = view.scene(&after, 1.0, size, after.tick as f32 / 30.0);
    let anchors = view.screens.anchors.clone();
    let mut focus_ms = vec![];
    let mut geometry_ms = vec![];
    let mut screens_ms = vec![];
    for _ in 0..120 {
        let t = Instant::now();
        assert!(view.live_focus(&after, 1.0, size, scene.camera).is_empty());
        focus_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        std::hint::black_box(view.geometry(&after, 1.0));
        geometry_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        view.screens.draw(
            &after,
            size,
            &anchors,
            &mut galaxy_render::Overlay::default(),
        );
        screens_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!(
        "{}",
        json!({"isolated_focus_ms":summary(focus_ms),"geometry_ms":summary(geometry_ms),
        "screen_layout_ms":summary(screens_ms)})
    );
}

//! Opt-in, headless screen/scene measurements against an operator-selected module.
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::Instant;

use galaxy_api::Registry;
use galaxy_module::module::Module;
use galaxy_render::glam::Vec2;
use galaxy_script::{ScriptSet, screens::Screen};
use galaxy_sim::{Session, scenario::InputsFile};
use galaxy_view::View;

#[test]
#[ignore = "operator-selected module and runtime performance measurement"]
fn screen_cost() {
    let module = Module::read(Path::new(&std::env::var("GALAXY_SCREEN_MODULE").unwrap())).unwrap();
    let inputs =
        InputsFile::read(Path::new(&std::env::var("GALAXY_SCREEN_INPUTS").unwrap())).unwrap();
    assert!(!inputs.needs_window());
    assert!(
        inputs.hot_loads.is_empty()
            && inputs.zooms.is_empty()
            && inputs.resizes.is_empty()
            && inputs.keys.is_empty(),
        "the benchmark accepts input events only"
    );
    let ticks: u64 = std::env::var("GALAXY_SCREEN_TICKS")
        .unwrap()
        .parse()
        .unwrap();
    let seed = std::env::var("GALAXY_SCREEN_SEED")
        .unwrap_or("7".into())
        .parse()
        .unwrap();
    let from = std::env::var("GALAXY_SCREEN_FROM")
        .unwrap_or("31".into())
        .parse()
        .unwrap();
    assert!(
        ticks > 0 && from <= ticks,
        "the measurement range must contain ticks"
    );
    let registry = Registry::new(&[&module]).unwrap();
    let scripts = ScriptSet::load(&module, &registry).unwrap();
    let mut screens: Vec<_> = scripts
        .scripts
        .iter()
        .filter(|s| s.screen)
        .map(|s| Screen::new(&module, &s.file, &registry))
        .collect();
    let mut session = Session::new(vec![module], seed).unwrap();
    let mut view = View::detached(session.modules(), 1);
    let by_tick = inputs.by_tick();
    let mut screen_ms = Vec::new();
    let mut scene_ms = Vec::new();
    let mut overlays = std::collections::hash_map::DefaultHasher::new();
    for tick in 1..=ticks {
        view.before_step(&session.world);
        session
            .step(by_tick.get(&tick).map(Vec::as_slice).unwrap_or(&[]))
            .unwrap();
        let start = Instant::now();
        for screen in &mut screens {
            screen.refresh(&session.world);
        }
        let screen_time = start.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        let scene = view.scene(
            &session.world,
            1.0,
            Vec2::new(1280.0, 720.0),
            tick as f32 / 30.0,
        );
        let scene_time = start.elapsed().as_secs_f64() * 1000.0;
        format!("{:?}", scene.overlay).hash(&mut overlays);
        if tick >= from {
            screen_ms.push(screen_time);
            scene_ms.push(scene_time);
        }
    }
    fn summary(mut samples: Vec<f64>) -> serde_json::Value {
        samples.sort_by(f64::total_cmp);
        serde_json::json!({"median": samples[samples.len()/2],
            "p95": samples[(samples.len()*95/100).min(samples.len()-1)],
            "max": samples.last().unwrap()})
    }
    println!(
        "{}",
        serde_json::json!({"screen_ms": summary(screen_ms),
        "scene_ms": summary(scene_ms), "ticks": ticks, "hash": session.world.hash(),
        "overlay_hash": format!("{:016x}", overlays.finish()),
        "screen_calls": screens.iter().map(|s| s.calls).sum::<u64>()})
    );
}

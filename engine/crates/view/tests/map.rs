//! `ui.map` painted headlessly: fog, explored ground, marks and the heading marker come
//! from the world every frame, without calling the screen's Luau again.
use std::path::Path;

use galaxy_module::module::{Module, ModuleFiles};
use galaxy_render::glam::Vec2;
use galaxy_render::screen_style::ScreenStyle;
use galaxy_render::{Overlay, PanelShape, UiLayer};
use galaxy_sim::Session;
use galaxy_sim::recording::{BuildStamp, Replay};
use galaxy_view::View;

const SIZE: Vec2 = Vec2::new(1280.0, 720.0);

fn module() -> Module {
    Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/map/chart")).unwrap()
}

/// The map's layer: the one whose first panel is the fog.
fn map_layer(overlay: &Overlay) -> &UiLayer {
    let fog = ScreenStyle::default().map_fog;
    overlay
        .widgets
        .iter()
        .find(|l| l.panels.first().is_some_and(|p| p.fill == fog))
        .expect("a map layer")
}

fn count(layer: &UiLayer, fill: galaxy_render::Color) -> usize {
    layer.panels.iter().filter(|p| p.fill == fill).count()
}

#[test]
fn the_chart_uncovers_where_the_ship_flew_without_running_luau_again() {
    let style = ScreenStyle::default();
    let mut session = Session::new(vec![module()], 1).unwrap();
    session.start_recording(BuildStamp::current());
    let mut view = View::detached(session.modules(), 1);
    session.step(&[]).unwrap();
    let first = view.scene(&session.world, 1.0, SIZE, 0.0).overlay;
    assert!(
        view.screens.problems.is_empty(),
        "{:?}",
        view.screens.problems
    );
    let layer = map_layer(&first);
    let start_clear = count(layer, style.map_clear);
    // Only the hub shows under fog (always); the rocks are far from the start.
    assert_eq!(count(layer, style.map_mark), 1);
    let me = layer
        .panels
        .iter()
        .rfind(|p| p.shape == PanelShape::Triangle)
        .unwrap();
    assert_eq!(me.fill, style.map_self);
    // The followed ship is at the map's centre.
    let clip = layer.clip.unwrap();
    assert!((me.rect.center() - clip.center()).length() < 1.0);
    for _ in 0..419 {
        view.before_step(&session.world);
        session.step(&[]).unwrap();
        view.scene(&session.world, 1.0, SIZE, 0.0);
    }
    let last = view.scene(&session.world, 1.0, SIZE, 0.0).overlay;
    assert!(
        view.screens.problems.is_empty(),
        "{:?}",
        view.screens.problems
    );
    assert_eq!(
        view.screens.calls(),
        1,
        "moving never calls the screen again"
    );
    let layer = map_layer(&last);
    assert!(
        count(layer, style.map_clear) > start_clear,
        "more ground is clear after flying"
    );
    assert!(count(layer, style.map_mark) >= 2, "a rock flown near shows");
    // The memory replays to the same world and the same paint.
    let recording = session.recording().unwrap();
    let mut replay = Replay::new(&recording).unwrap();
    while replay.step().unwrap() {}
    assert_eq!(replay.session.world.hash(), session.world.hash());
    let mut rebuilt = View::detached(replay.session.modules(), 1);
    let again = rebuilt.scene(&replay.session.world, 1.0, SIZE, 0.0).overlay;
    assert_eq!(again, last);
}

fn screen(start: &str, code: &str) -> (Session, View) {
    let mut files = ModuleFiles::default();
    files.0.insert(
        "manifest.toml".into(),
        "id = 'test'\nversion = '0.1.0'\nmin_engine_version = '0.9.0'\n\
         scripts = ['screen.luau']\nwrites = ['engine.position', 'engine.explorer', 'engine.explored']\n"
            .into(),
    );
    files.0.insert("start.toml".into(), start.into());
    files.0.insert("screen.luau".into(), code.into());
    let module = Module::from_files(Path::new("test"), files).unwrap();
    let session = Session::new(vec![module], 1).unwrap();
    let view = View::detached(session.modules(), 1);
    (session, view)
}

fn problems(start: &str, map: &str) -> Vec<String> {
    let code = format!("return {{screen=function(_,ui) return ui.map {{ {map} }} end}}");
    let (mut s, mut v) = screen(start, &code);
    s.step(&[]).unwrap();
    v.scene(&s.world, 1.0, SIZE, 0.0);
    v.screens.problems.clone()
}

#[test]
fn bad_maps_are_reported_by_name() {
    let start = "[entities.ship]\n\"engine.position\" = {}\n\
                 [entities.rock]\n\"engine.position\" = { x = 5.0 }\n";
    let ok = problems(
        start,
        "width=200, center={x=0,y=0}, span=100, marks={{entity='rock'}}",
    );
    assert!(ok.is_empty(), "{ok:?}");
    for (map, needle) in [
        (
            "width=200, follow='ship', memory='ship', span=100",
            "map memory `test.ship` has no engine.explored or engine.explorer",
        ),
        (
            "width=200, follow='ship', span=100, marks={{entity='gone'}}",
            "mark `test.gone` doesn't exist",
        ),
        ("width=200, follow='ship', span=0", "span must be above 0"),
        ("width=200, span=50", "needs follow"),
        (
            "width=200, follow='ship', center={x=1,y=2}, span=50",
            "follow or center, not both",
        ),
        (
            "width=200, follow='ship', span=50, marks={{entity='rock', size=3}}",
            "marks[1]: unknown property `size`",
        ),
        (
            "width=200, follow='ship', span=50, marks={{entity='rock', icon='nope'}}",
            "missing module icon `nope`",
        ),
        (
            "width=200, follow='ship', span=50, marks={{entity='rock', look='absent'}}",
            "unknown look `absent`",
        ),
    ] {
        let found = problems(start, map);
        assert!(
            found.iter().any(|p| p.contains(needle)),
            "{map}: {found:?} lacks {needle}"
        );
    }
    let code = "return {screen=function(_,ui) return ui.text { text='x', span=5 } end}";
    let (_, mut v) = screen(start, code);
    let s = Session::new(vec![], 1).unwrap();
    v.scene(&s.world, 1.0, SIZE, 0.0);
    assert!(
        v.screens
            .problems
            .iter()
            .any(|p| p.contains("span goes on a ui.map")),
        "{:?}",
        v.screens.problems
    );
}

#[test]
fn a_memory_set_by_hand_is_what_the_map_clears() {
    // One explored cell, right of centre: one clear rectangle, a mark there shows and a
    // mark in the fog doesn't.
    let start = "[entities.ship]\n\"engine.position\" = {}\n\
                 \"engine.explored\" = { cell = 10.0, cells = \"0:2+1\" }\n\
                 [entities.seen]\n\"engine.position\" = { x = 25.0, y = 5.0 }\n\
                 [entities.hidden]\n\"engine.position\" = { x = -25.0, y = 5.0 }\n";
    let code = "return {screen=function(_,ui) return ui.map { width=200, height=100, \
                center={x=0,y=0}, memory='ship', span=100, \
                marks={{entity='seen'},{entity='hidden'}} } end}";
    let (s, mut v) = screen(start, code);
    let overlay = v.scene(&s.world, 1.0, SIZE, 0.0).overlay;
    assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
    let style = ScreenStyle::default();
    let layer = map_layer(&overlay);
    let clip = layer.clip.unwrap();
    let clear: Vec<_> = layer
        .panels
        .iter()
        .filter(|p| p.fill == style.map_clear && p.radius == 0.0)
        .collect();
    assert_eq!(clear.len(), 1);
    // 2 px a world unit: cell x 20..30, y 0..10 is 40..60 px right of centre, 0..20 up.
    let r = clear[0].rect;
    let centre = clip.center();
    assert!((r.x - (centre.x + 40.0)).abs() < 0.01, "{r:?} {clip:?}");
    assert!((r.y - (centre.y - 20.0)).abs() < 0.01, "{r:?} {clip:?}");
    assert!((r.w - 21.0).abs() < 0.01 && (r.h - 21.0).abs() < 0.01);
    assert_eq!(count(layer, style.map_mark), 1);
}

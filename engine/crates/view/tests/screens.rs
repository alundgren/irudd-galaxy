use galaxy_module::module::{Module, ModuleFiles};
use galaxy_render::glam::Vec2;
use galaxy_sim::{
    Session,
    recording::{BuildStamp, Replay},
};
use galaxy_view::View;
use std::path::Path;

fn module() -> Module {
    Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/screens")).unwrap()
}

#[test]
fn screen_clicks_replay_and_anchors_move_without_luau_per_frame() {
    let module = module();
    let mut session = Session::new(vec![module], 1).unwrap();
    session.start_recording(BuildStamp::current());
    let mut view = View::detached(session.modules(), 1);
    let hash = session.world.hash();
    let first = view.scene(&session.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert_eq!(session.world.hash(), hash);
    assert!(
        view.screens.problems.is_empty(),
        "{:?}",
        view.screens.problems
    );
    let panel = &view.screens.boxes[0];
    let button = &panel.children[3].children[0];
    let point = button.rect.center();
    let anchor = panel.children[4].rect;
    let click = view.screens.click(point, 1).unwrap();
    assert_eq!(click.source, "ui");
    session.step(&[click]).unwrap();
    for _ in 0..20 {
        view.before_step(&session.world);
        session.step(&[]).unwrap();
        view.scene(&session.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    }
    assert_eq!(view.screens.calls(), 2, "only the counter read changed");
    assert_ne!(view.screens.boxes[0].children[4].rect, anchor);
    assert!(view.screens.boxes[0].children[1].widget.text.ends_with('1'));
    let final_scene = view.scene(&session.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert_ne!(first.overlay, final_scene.overlay);
    let recording = session.recording().unwrap();
    let mut replay = Replay::new(&recording).unwrap();
    while replay.step().unwrap() {}
    assert_eq!(replay.session.world.hash(), session.world.hash());
    let mut rebuilt = View::detached(replay.session.modules(), 1);
    let scene = rebuilt.scene(&replay.session.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert_eq!(
        scene.overlay, final_scene.overlay,
        "a fresh audit/film view rebuilds the screen"
    );
}

fn screen(code: &str) -> (Session, View) {
    let mut files = ModuleFiles::default();
    files.0.insert("manifest.toml".into(), "id = 'test'\nversion = '0.1.0'\nmin_engine_version = '0.3.0'\nscripts = ['screen.luau']\nevents = ['click']\n".into());
    files.0.insert("screen.luau".into(), code.into());
    let module = Module::from_files(Path::new("test"), files).unwrap();
    let session = Session::new(vec![module], 1).unwrap();
    let view = View::detached(session.modules(), 1);
    (session, view)
}

#[test]
fn widget_order_interleaves_text_and_panels_and_blocks_covered_clicks() {
    let (s, mut v) = screen(
        r#"return {screen=function(_,ui)
        return ui.column {height=300,children={
            ui.button {text="Behind",on="click",width=200},
            ui.panel {x=0,y=0,width=200,height=60,children={ui.text{text="Above"}}},
        }}
    end}"#,
    );
    let scene = v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    let layers = &scene.overlay.widgets;
    assert_eq!(layers[0].texts[0].text, "Behind");
    assert!(!layers[1].panels.is_empty());
    assert_eq!(layers[2].texts[0].text, "Above");
    assert!(v.screens.click(Vec2::new(50.0, 45.0), 1).is_none());
    assert!(v.screens.problems.iter().any(|p| p.contains("overlaps")));
}

#[test]
fn readability_names_paths_and_screen_api_cannot_mutate_the_world() {
    let (s, mut v) = screen(
        r#"return {screen=function(_,ui)
        return ui.panel{width=120,height=35,children={ui.text{id="tiny",text="Measured text",size=8}}}
    end}"#,
    );
    v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert!(
        v.screens
            .problems
            .iter()
            .any(|p| p.contains("/tiny[1]") && p.contains("16 logical"))
    );
    let (s, mut v) = screen("return {screen=function(ctx,_) ctx.put('x','x',{}) end}");
    let hash = s.world.hash();
    v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert!(!v.screens.problems.is_empty());
    assert_eq!(s.world.hash(), hash);
}

#[test]
fn missing_reads_and_query_membership_rebuild_a_hidden_screen() {
    let (mut s, mut v) = screen(
        r#"return {
        on={click=function(_,ctx) ctx.put("new","engine.anchor",{name="test"}) end},
        screen=function(ctx,ui)
            local absent = ctx.get("new","engine.anchor")
            local all = ctx.query("engine.anchor")
            if absent == nil then return nil end
            return ui.text{text="Anchors: " .. #all}
        end,
    }"#,
    );
    // Grant the test module its one engine write by reloading its manifest.
    let mut module = s.modules().next().unwrap().clone();
    module
        .files
        .0
        .get_mut("manifest.toml")
        .unwrap()
        .push_str("writes = ['engine.anchor']\n");
    let module = Module::from_files(Path::new("test"), module.files).unwrap();
    s.hot_load(module).unwrap();
    v.reload(None, s.modules());
    v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
    assert!(v.screens.boxes.is_empty());
    s.step(&[galaxy_sim::Input {
        tick: 1,
        source: "ui".into(),
        name: "test.click".into(),
        payload: Default::default(),
    }])
    .unwrap();
    v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert_eq!(v.screens.boxes[0].widget.text, "Anchors: 1");
    assert_eq!(v.screens.calls(), 2);
}

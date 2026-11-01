use galaxy_module::module::{Module, ModuleFiles};
use galaxy_render::glam::Vec2;
use galaxy_sim::{
    Session,
    recording::{BuildStamp, Replay},
    scenario::{Click, ClickTarget, InputsFile},
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
fn separate_screens_report_covered_controls_with_both_paths() {
    for wrapper in ["ui.panel", "ui.column"] {
        let (s, _) = screen(
            r#"return {screen=function(_,ui)
            return ui.button {width=280,text="Continue",on="click"}
        end}"#,
        );
        let mut module = s.modules().next().unwrap().clone();
        let manifest = module.files.0.get_mut("manifest.toml").unwrap();
        *manifest = manifest.replace("['screen.luau']", "['screen.luau', 'other.luau']");
        module.files.0.insert(
            "other.luau".into(),
            format!(
                r#"return {{screen=function(_,ui)
            return {wrapper} {{width=280,height=80,children={{ui.text{{text="Other screen"}}}}}}
        end}}"#
            ),
        );
        let module = Module::from_files(Path::new("test"), module.files).unwrap();
        let s = Session::new(vec![module], 1).unwrap();
        let mut v = View::detached(s.modules(), 1);
        v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
        assert!(
            v.screens
                .problems
                .iter()
                .any(|p| p.contains("test:screen.luau/screen")
                    && p.contains("test:other.luau/screen")
                    && p.contains("overlaps")),
            "{:?}",
            v.screens.problems
        );
    }
}

#[test]
fn malformed_widget_fields_name_the_field_instead_of_dropping_content() {
    for (field, value) in [
        ("children", r#"ui.button {text="Continue",on="click"}"#),
        ("children", r#"{first=ui.text{text="Lost"}}"#),
        (
            "children",
            r#"{[1]=ui.text{text="One"},[3]=ui.text{text="Lost"}}"#,
        ),
        ("children", "42"),
        ("text", "{}"),
        ("id", "{}"),
        ("anchor", "{}"),
        ("on", "{}"),
        ("payload", "42"),
        ("width", "{}"),
        ("text", "42"),
    ] {
        let (s, mut v) = screen(&format!(
            "return {{screen=function(_,ui) return ui.panel {{{field}={value}}} end}}"
        ));
        v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
        assert!(
            v.screens.problems.iter().any(|p| p.contains(field)),
            "{field}={value}: {:?}",
            v.screens.problems
        );
        assert!(v.screens.boxes.is_empty());
    }
}

#[test]
fn guide_bot_clicks_a_moving_widget_by_name_and_path_and_replays() {
    let topic = galaxy_module::help::TOPICS
        .iter()
        .find(|t| t.name == "screens")
        .unwrap();
    let mut files = ModuleFiles::default();
    let dir = tempfile::tempdir().unwrap();
    for step in topic.recipes[0].steps {
        // Exercise the text authors actually see, including the guide's line wrapping.
        let step = galaxy_module::help::wrap(step, 86).join("\n");
        if let Some(rest) = step.strip_prefix("Create example/") {
            let (file, text) = rest.split_once(" with:\n").unwrap();
            files.0.insert(file.into(), text.into());
        }
        if let Some(text) = step.strip_prefix("Write clicks.toml with:\n") {
            std::fs::write(dir.path().join("clicks.toml"), text).unwrap();
        }
    }
    let script = InputsFile::read(&dir.path().join("clicks.toml")).unwrap();
    let module = Module::from_files(Path::new("example"), files).unwrap();
    let mut s = Session::new(vec![module], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    let old_point = v.screens.boxes[0].children[1].rect.center();
    for tick in 1..=200 {
        v.before_step(&s.world);
        v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
        let mut inputs = Vec::new();
        if tick == 20 {
            let click = v.screens.bot_click(&script.clicks[0]).unwrap();
            assert_eq!(click.source, "ui");
            assert_eq!(click.name, "example.activate");
            inputs.push(click);
        }
        if tick == 200 {
            let b = &v.screens.boxes[0].children[1];
            assert_ne!(b.rect.center(), old_point);
            assert!(
                v.screens.click(old_point, tick).is_none(),
                "old coordinates miss the moving button"
            );
            let click = v
                .screens
                .bot_click(&Click {
                    tick,
                    target: ClickTarget::Widget(b.widget.path.clone()),
                })
                .unwrap();
            assert_eq!(click, v.screens.click(b.rect.center(), tick).unwrap());
            inputs.push(click);
        }
        assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
        s.step(&inputs).unwrap();
    }
    assert_eq!(v.screens.calls(), 1, "anchor moves without re-running Luau");
    assert_eq!(
        s.world
            .facts
            .iter()
            .filter(|f| f.kind == "example.clicked")
            .count(),
        2
    );
    let recording = s.recording().unwrap();
    let mut replay = Replay::new(&recording).unwrap();
    while replay.step().unwrap() {}
    assert_eq!(replay.session.world.hash(), s.world.hash());
    let mut rebuilt = View::detached(replay.session.modules(), 1);
    assert_eq!(
        rebuilt
            .scene(&replay.session.world, 1.0, Vec2::new(1280.0, 720.0), 0.0)
            .overlay,
        v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0)
            .overlay,
    );
}

#[test]
fn widget_targets_handle_root_ids_ambiguity_missing_and_covered_buttons() {
    let (s, mut v) = screen(
        r#"return {screen=function(_,ui)
        return ui.button{id="root",text="Continue",on="click"}
    end}"#,
    );
    v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    let named = |name: &str| Click {
        tick: 1,
        target: ClickTarget::Widget(name.into()),
    };
    assert!(v.screens.bot_click(&named("root")).is_ok());
    let point = v.screens.boxes[0].rect.center();
    assert_eq!(
        v.screens.bot_click(&named("root")).unwrap(),
        v.screens
            .bot_click(&Click {
                tick: 1,
                target: ClickTarget::Point {
                    x: point.x,
                    y: point.y
                },
            })
            .unwrap()
    );
    assert!(
        v.screens
            .bot_click(&named("absent"))
            .unwrap_err()
            .contains("missing")
    );
    let (s, mut v) = screen(
        r#"return {screen=function(_,ui)
        return ui.column{children={
            ui.button{id="same",text="One",on="click"},
            ui.button{id="same",text="Two",on="click"},
            ui.panel{id="cover",x=0,y=0,height=60},
        }}
    end}"#,
    );
    v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert!(
        v.screens
            .bot_click(&named("same"))
            .unwrap_err()
            .contains("ambiguous")
    );
    assert!(
        v.screens
            .bot_click(&named("test:screen.luau/screen/same[2]"))
            .is_ok()
    );
    let error = v
        .screens
        .bot_click(&named("test:screen.luau/screen/same[1]"))
        .unwrap_err();
    assert!(
        error.contains("same[1]") && error.contains("cover[3]"),
        "{error}"
    );
    assert!(
        v.screens
            .bot_click(&named("cover"))
            .unwrap_err()
            .contains("not a button")
    );
}

#[test]
fn unbacked_text_gets_paper_but_text_inside_a_panel_reuses_it() {
    for root in [
        r#"ui.text{text="Read this important message."}"#,
        r#"ui.column{children={ui.text{text="Read this important message."}}}"#,
        r#"ui.panel{width=200,children={ui.text{x=240,text="Read this important message."}}}"#,
    ] {
        let (s, mut v) = screen(&format!(
            "return {{screen=function(_,ui) return {root} end}}"
        ));
        let scene = v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
        let label = scene
            .overlay
            .widgets
            .iter()
            .find(|l| !l.texts.is_empty())
            .unwrap();
        assert_eq!(
            label.panels.len(),
            1,
            "unbacked text needs a surface: {root}"
        );
        assert_eq!(label.panels[0].fill, galaxy_render::defaults::PAPER);
        assert_eq!(label.texts[0].color, galaxy_render::defaults::INK);
    }
    let (s, mut v) = screen(
        r#"return {screen=function(_,ui)
        return ui.panel{children={ui.column{children={ui.text{text="Backed"}}}}}
    end}"#,
    );
    let scene = v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert_eq!(
        scene
            .overlay
            .widgets
            .iter()
            .map(|l| l.panels.len())
            .sum::<usize>(),
        1
    );
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

#[test]
fn a_wrap_width_narrower_than_one_glyph_is_unreadable() {
    let (s, mut v) = screen(
        r#"return {screen=function(_,ui)
        return ui.text {text="W",width=1}
    end}"#,
    );
    v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert!(
        v.screens
            .problems
            .iter()
            .any(|p| p.contains("text is clipped")),
        "{:?}",
        v.screens.problems
    );
}

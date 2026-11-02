use galaxy_module::module::Module;
use galaxy_render::{glam::Vec2, screen_style::ScreenStyle};
use galaxy_sim::{
    Session,
    recording::{BuildStamp, Replay},
    scenario::InputsFile,
};
use galaxy_view::{View, screens::Boxed};
use std::path::Path;

fn module(name: &str) -> Module {
    Module::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/play/{name}/styled")),
    )
    .unwrap()
}

fn boxes(nodes: &[Boxed]) -> Vec<(String, galaxy_render::Rect)> {
    nodes
        .iter()
        .flat_map(|b| {
            let mut out = vec![(b.widget.path.clone(), b.rect)];
            out.extend(boxes(&b.children));
            out
        })
        .collect()
}

#[test]
fn both_looks_keep_layout_inputs_motion_world_hashes_and_rebuilt_notes() {
    let a = module("screen-light");
    let b = module("screen-dark");
    for (path, text) in &a.files.0 {
        if path != "style.toml" {
            assert_eq!(Some(text.as_str()), b.files.get(path), "{path}");
        }
    }
    let bot = InputsFile::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/screen-style-inputs.toml"),
    )
    .unwrap();
    let mut sessions = [
        Session::new(vec![a], 1).unwrap(),
        Session::new(vec![b], 1).unwrap(),
    ];
    for s in &mut sessions {
        s.start_recording(BuildStamp::current());
    }
    let mut views = sessions.each_ref().map(|s| View::detached(s.modules(), 1));
    for tick in 1..=140 {
        let mut inputs = [vec![], vec![]];
        let mut overlays = vec![];
        for i in 0..2 {
            let (s, v) = (&mut sessions[i], &mut views[i]);
            v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
            for p in bot.pointers.iter().filter(|p| p.tick == tick) {
                if let Some(input) = v.screens.bot_pointer(p).unwrap() {
                    inputs[i].push(input);
                }
            }
            v.before_step(&s.world);
            s.step(&inputs[i]).unwrap();
            let scene = v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
            assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
            assert!(v.assets.problems.is_empty(), "{:?}", v.assets.problems);
            s.record_presentation(serde_json::to_value(&v.screens.presentation).unwrap());
            if let Some((_, text)) = bot.notes.iter().find(|n| n.0 == tick) {
                let moment = v.moment(1.0, 0.0, (1280, 720), 1.0).unwrap();
                s.note(text, Some(serde_json::to_value(&moment).unwrap()), None);
                let rec = s.recording().unwrap();
                let mut replay = Replay::new(&rec).unwrap();
                while replay.step().unwrap() {}
                let mut rebuilt = View::detached(replay.session.modules(), 1);
                rebuilt.screens.presentation = moment.screens;
                assert_eq!(
                    rebuilt
                        .scene(&replay.session.world, 1.0, Vec2::new(1280.0, 720.0), 0.0)
                        .overlay,
                    scene.overlay
                );
            }
            overlays.push(scene.overlay);
        }
        assert_eq!(inputs[0], inputs[1]);
        assert_eq!(sessions[0].world.hash(), sessions[1].world.hash());
        assert_eq!(views[0].screens.presentation, views[1].screens.presentation);
        assert_eq!(
            boxes(&views[0].screens.boxes),
            boxes(&views[1].screens.boxes)
        );
        assert_ne!(overlays[0], overlays[1]);
    }
    for s in &sessions {
        assert_eq!(
            s.world
                .component("styled.state", "styled.state")
                .unwrap()
                .fields["drops"],
            1
        );
        let rec = s.recording().unwrap();
        let mut replay = Replay::new(&rec).unwrap();
        while replay.step().unwrap() {}
        assert_eq!(s.world.hash(), replay.session.world.hash());
    }
}

#[test]
fn reload_changes_paint_without_simulation_changes_and_replays_style_at_same_tick() {
    let mut s = Session::new(vec![module("screen-light")], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    s.start_recording(BuildStamp::current());
    s.step(&[]).unwrap();
    let hash = s.world.hash();
    let before = v
        .scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0)
        .overlay;
    let before_moment = v.moment(1.0, 0.0, (1280, 720), 1.0).unwrap();
    let next = module("screen-dark");
    s.hot_load(next).unwrap();
    v.reload(None, s.modules());
    assert_eq!(hash, s.world.hash());
    assert_eq!(v.note_scene(&s.world, &before_moment).overlay, before);
    assert_ne!(
        before,
        v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0)
            .overlay
    );
    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    assert!(!replay.modules_changed);
    replay.step().unwrap();
    assert!(replay.modules_changed);
    assert_eq!(hash, replay.session.world.hash());
    let mut rebuilt = View::detached(replay.session.modules(), 1);
    assert_eq!(
        rebuilt
            .scene(&replay.session.world, 1.0, Vec2::new(1280.0, 720.0), 0.0)
            .overlay,
        v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0)
            .overlay
    );
    let mut default = module("screen-light");
    default.files.0.remove("style.toml");
    let mut v = View::detached([&default], 1);
    v.scene(&s.world, 1.0, Vec2::new(1280.0, 720.0), 0.0);
    assert_eq!(v.screens.style, ScreenStyle::default());
}

#[test]
fn invalid_styles_name_fields_and_multiple_modules_use_id_order() {
    for (setting, field) in [
        ("text = '#eeeeee'", "text"),
        ("handle_min = 8", "handle_min"),
        ("handle_width = 8", "handle_width"),
        ("target_min = 8", "target_min"),
        ("paper = '#ffffff80'", "paper"),
    ] {
        let mut m = module("screen-light");
        m.files
            .0
            .insert("style.toml".into(), format!("[screen]\n{setting}"));
        let v = View::detached([&m], 1);
        assert!(v.assets.problems.join(" ").contains(field));
        assert_eq!(v.screens.style, ScreenStyle::default());
    }
    let a = module("screen-light");
    let mut b = module("screen-dark");
    b.manifest.id = "zz".into();
    let assets = galaxy_view::assets::Assets::load([&b, &a]);
    assert_eq!(assets.screen.panel.to_hex(), "#18222c");
    assert_eq!(assets.conflicts[0].table, "screen");
    assert_eq!(assets.conflicts[0].modules, ["styled", "zz"]);
}

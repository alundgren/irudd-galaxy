use galaxy_module::module::{Module, ModuleFiles};
use galaxy_render::glam::Vec2;
use galaxy_sim::{
    BuildStamp, Replay, Session,
    scenario::{Click, ClickTarget, InputsFile},
};
use galaxy_view::View;
use std::path::Path;

fn size() -> Vec2 {
    Vec2::new(1280.0, 720.0)
}

#[test]
fn widget_and_walking_speaker_hovers_reconstruct_without_world_changes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play");
    let module = Module::read(&root.join("life/demo")).unwrap();
    let mut s = Session::new(vec![module], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    let bot = InputsFile::read(&root.join("hovers-inputs.toml")).unwrap();
    let mut notes = Vec::new();
    let mut moving = Vec::new();
    for tick in 1..=96 {
        v.before_step(&s.world);
        v.scene(&s.world, 1.0, size(), 0.0);
        let hash = s.world.hash();
        for p in bot.pointers.iter().filter(|p| p.tick == tick) {
            assert!(v.bot_pointer(&s.world, p).unwrap().is_none());
        }
        assert_eq!(hash, s.world.hash());
        let inputs: Vec<_> = bot
            .clicks
            .iter()
            .filter(|c| c.tick == tick)
            .map(|c| v.bot_talk_click(&s.world, c).unwrap())
            .collect();
        s.step(&inputs).unwrap();
        let scene = v.scene(&s.world, 0.5, size(), tick as f32 / 30.0);
        let p = v.screens.presented.pointer.map(Vec2::from);
        let look = p.map(|p| v.look(&s.world, 0.5, size(), p));
        if (30..=85).contains(&tick) {
            let look = look.as_ref().unwrap();
            assert_eq!(look["being"], "life.walker", "tick {tick}: {look}");
            assert_eq!(look["hover"]["readable"], true, "tick {tick}: {look}");
            moving.push(look["hover"]["rect"][0].as_f64().unwrap());
        }
        s.record_presentation(serde_json::to_value(&v.screens.presented).unwrap());
        if let Some((_, text)) = bot.notes.iter().find(|n| n.0 == tick) {
            let moment = v.moment(0.5, tick as f32 / 30.0, (1280, 720), 1.0).unwrap();
            s.note(text, Some(serde_json::to_value(&moment).unwrap()), None);
            notes.push((tick, moment, scene.overlay, look.unwrap()));
        }
    }
    assert!(moving.windows(2).any(|p| (p[1] - p[0]).abs() > 0.01));
    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    let mut rebuilt = View::detached(replay.session.modules(), 1);
    for (tick, moment, overlay, look) in &notes {
        while replay.session.world.tick < *tick {
            rebuilt.before_step(&replay.session.world);
            replay.step().unwrap();
        }
        assert_eq!(
            rebuilt.note_scene(&replay.session.world, moment).overlay,
            *overlay,
            "note tick {tick}"
        );
        assert_eq!(
            rebuilt.look(
                &replay.session.world,
                moment.alpha,
                size(),
                moment.screens.pointer.unwrap().into()
            ),
            *look
        );
    }
    while replay.step().unwrap() {}
    assert_eq!(replay.session.world.hash(), s.world.hash());
    assert!(notes[1].3["hover"].is_null());
    assert!(notes[6].3["hover"].is_null());
}

fn screen(code: &str) -> (Session, View) {
    let mut files = ModuleFiles::default();
    files.0.insert("manifest.toml".into(),"id='example'\nversion='0.1.0'\nmin_engine_version='0.4.0'\nscripts=['screen.luau']\nevents=['click']".into());
    files.0.insert(
        "screen.luau".into(),
        format!("return {{screen=function(_,ui) {code} end}}"),
    );
    let s = Session::new(
        vec![Module::from_files(Path::new("example"), files).unwrap()],
        1,
    )
    .unwrap();
    let v = View::detached(s.modules(), 1);
    (s, v)
}

#[test]
fn check_reaches_scrolled_entries_and_uncovered_edges_and_reports_bad_hovers() {
    let (s, mut v) = screen(
        r#"
        local rows={}
        for i=1,12 do table.insert(rows,ui.text{id="row"..i,text="Entry "..i,hover=if i==12 then "" else "Hover "..i}) end
        return ui.column{width=500,children={
            ui.scroll{id="list",height=150,children=rows},
            ui.panel{id="under",x=600,y=200,width=200,height=100,hover="Exposed edge"},
            ui.panel{id="over",x=650,y=200,width=200,height=100}
        }}
    "#,
    );
    let hash = s.world.hash();
    v.hover_checks = Some(Default::default());
    v.scene(&s.world, 1.0, size(), 0.0);
    assert_eq!(hash, s.world.hash());
    let reports = v
        .hover_checks
        .as_ref()
        .unwrap()
        .values()
        .collect::<Vec<_>>();
    for i in 1..=12 {
        assert!(
            reports.iter().any(|r| r["widget"]
                .as_str()
                .unwrap_or("")
                .contains(&format!("/row{i}["))),
            "missing row {i}"
        );
    }
    assert!(reports.iter().any(|r| r["hover"]["text"] == "Exposed edge"));
    assert!(
        reports
            .iter()
            .any(|r| r["hover"]["readable"] == false && r["hover"]["text"] == "")
    );
    assert!(v.screens.presentation.scroll.values().all(|v| *v == 0.0));
    assert!(
        v.look(&s.world, 1.0, size(), Vec2::new(710.0, 250.0))["hover"].is_null(),
        "opaque panel blocks the hover beneath"
    );
}

#[test]
fn hover_readability_uses_font_and_viewport_and_capture_clears_it() {
    let (s, mut v) =
        screen(r#"return ui.text{width=200,text="Target",hover=string.rep("Long label ",800)}"#);
    v.scene(&s.world, 1.0, size(), 0.0);
    let p = v.screens.boxes[0].rect.center();
    let look = v.look(&s.world, 1.0, size(), p);
    assert_eq!(look["hover"]["readable"], false);
    v.screens.pointer_move(p, 1);
    let shown = v.scene(&s.world, 1.0, size(), 0.0);
    v.screens.cancel_pointer(1);
    let cleared = v.scene(&s.world, 1.0, size(), 0.0);
    assert!(shown.overlay.widgets.len() > cleared.overlay.widgets.len());
}

#[test]
fn container_hovers_and_empty_model_nodes_are_reachable() {
    let (s, mut v) = screen(
        r#"return ui.column{width=200,height=120,hover="Container help",children={ui.text{text="Child"}}}"#,
    );
    v.scene(&s.world, 1.0, size(), 0.0);
    assert_eq!(
        v.look(&s.world, 1.0, size(), Vec2::new(40.0, 35.0))["hover"]["text"],
        "Container help"
    );
    assert_eq!(
        v.look(&s.world, 1.0, size(), Vec2::new(40.0, 110.0))["hover"]["text"],
        "Container help"
    );

    let mut m =
        Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/life/demo"))
            .unwrap();
    let mut model: serde_json::Value =
        serde_json::from_str(m.files.get("models/being.gltf").unwrap()).unwrap();
    model["nodes"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"name":"tag","translation":[0,3,0]}));
    model["scenes"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(2));
    m.files
        .0
        .insert("models/being.gltf".into(), model.to_string());
    m.files.0.insert("scripts/main.luau".into(),"return {screen=function(_,ui) return ui.hover{id='tag',anchor='being.life.walker.tag',text='Empty node label'} end}".into());
    let m = Module::from_files(Path::new("demo"), m.files).unwrap();
    let s = Session::new(vec![m], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    v.scene(&s.world, 1.0, size(), 0.0);
    let p = v.screens.anchors["being.life.walker.tag"];
    let look = v.look(&s.world, 1.0, size(), p);
    assert_eq!(look["entity"], "life.walker");
    assert_eq!(look["part"], "tag");
    assert_eq!(look["hover"]["text"], "Empty node label");
    v.hover_checks = Some(Default::default());
    v.scene(&s.world, 1.0, size(), 0.0);
    assert!(
        v.hover_checks
            .unwrap()
            .values()
            .any(|v| v["hover"]["text"] == "Empty node label")
    );
}

#[test]
fn transparent_container_hover_preserves_widget_and_speaker_clicks() {
    let mut speaker_inputs = Vec::new();
    let mut widget_inputs = Vec::new();
    for hover in ["", "hover='Scene help',"] {
        let mut module =
            Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/life/demo"))
                .unwrap();
        module.files.0.insert(
            "scripts/main.luau".into(),
            format!(
                "return {{screen=function(_,ui) return ui.column{{width=1240,height=680,{hover}children={{}}}} end}}"
            ),
        );
        let s = Session::new(vec![module], 1).unwrap();
        let mut v = View::detached(s.modules(), 1);
        v.scene(&s.world, 1.0, size(), 0.0);
        speaker_inputs.push(
            v.bot_talk_click(
                &s.world,
                &Click {
                    tick: 2,
                    target: ClickTarget::Widget("speaker.life.walker".into()),
                },
            )
            .unwrap(),
        );
        if !hover.is_empty() {
            assert_eq!(
                v.look(&s.world, 1.0, size(), Vec2::new(40.0, 110.0))["hover"]["text"],
                "Scene help"
            );
        }

        let (s, mut v) = screen(&format!(
            "return ui.column{{width=300,height=200,children={{
                ui.button{{id='button',x=0,y=0,width=200,height=100,text='Click',on='click'}},
                ui.column{{x=0,y=0,width=300,height=200,{hover}children={{}}}}
            }}}}"
        ));
        v.scene(&s.world, 1.0, size(), 0.0);
        widget_inputs.push(
            v.screens
                .bot_click(&Click {
                    tick: 2,
                    target: ClickTarget::Widget("button".into()),
                })
                .unwrap(),
        );
        assert_eq!(
            v.screens.click(Vec2::new(100.0, 70.0), 2).as_ref(),
            widget_inputs.last()
        );
    }
    assert_eq!(speaker_inputs[0], speaker_inputs[1]);
    assert_eq!(widget_inputs[0], widget_inputs[1]);
}

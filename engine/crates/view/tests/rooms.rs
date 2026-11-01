use galaxy_api::engine::{POSITION, SCRIPT_FAILED, TRAVEL};
use galaxy_module::module::Module;
use galaxy_render::glam::Vec2;
use galaxy_sim::scenario::{Click, ClickTarget};
use galaxy_sim::{BuildStamp, Session, recording::Replay, save, travel};
use galaxy_view::{View, travel::Cut};
use std::{collections::BTreeMap, path::Path};

fn module() -> Module {
    Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/rooms/demo"))
        .unwrap()
}
fn size() -> Vec2 {
    Vec2::new(1280.0, 720.0)
}
fn click(s: &mut Session, v: &mut View, widget: &str) {
    v.scene(&s.world, 1.0, size(), 0.0);
    let input = travel::arrival(&s.world, true).unwrap_or_else(|| {
        v.screens
            .bot_click(&Click {
                tick: s.world.tick + 1,
                target: ClickTarget::Widget(widget.into()),
            })
            .unwrap()
    });
    v.before_step(&s.world);
    s.step(&[input]).unwrap();
}

#[test]
fn rooms_travel_arrival_save_replay_and_fresh_views_agree() {
    let mut s = Session::new(vec![module()], 7).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 7);
    assert_eq!(
        s.world.component("rooms.first", POSITION).unwrap().fields["x"],
        -2.0
    );
    let findings = v.check_rooms(&s.world, size());
    assert!(findings.is_empty(), "{findings:?}");
    click(&mut s, &mut v, "enter_a");
    for _ in 0..12 {
        v.before_step(&s.world);
        s.step(&[]).unwrap();
    }
    let cut = Cut::step(&s.world, 1.0).unwrap();
    assert_eq!(cut.opacity(), 1.0);
    assert!(!cut.reverse);
    assert!(travel::arrival(&s.world, false).is_none());
    let frame = v.scene(&s.world, 0.4, size(), 3.0);
    let recording = s.recording().unwrap();
    let mut replay = Replay::new(&recording).unwrap();
    let mut rebuilt = View::detached(replay.session.modules(), 7);
    loop {
        rebuilt.before_step(&replay.session.world);
        if !replay.step().unwrap() {
            break;
        }
    }
    assert_eq!(s.world.hash(), replay.session.world.hash());
    let redrawn = rebuilt.scene(&replay.session.world, 0.4, size(), 3.0);
    assert_eq!(redrawn.overlay, frame.overlay);
    assert_eq!(redrawn.camera, frame.camera);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("room.db");
    save::write(&path, &s.world, s.modules(), &BuildStamp::current()).unwrap();
    let (_, world) = save::read(&path).unwrap();
    let resumed = Session::resume(vec![module()], world).unwrap();
    assert_eq!(resumed.world.hash(), s.world.hash());
    // Any click lands, without activating the covered button underneath it.
    click(&mut s, &mut v, "leave");
    assert_eq!(
        s.world.component("rooms.player", TRAVEL).unwrap().fields["active"],
        false
    );
    assert_eq!(
        s.world
            .component("rooms.state", "rooms.visits")
            .unwrap()
            .fields["count"],
        1
    );
    let scene = v.scene(&s.world, 1.0, size(), 3.0);
    assert_eq!(scene.meshes.len(), 3, "only this room's set and two beings");
    let mut anchors = BTreeMap::new();
    v.place_anchors(&s.world, 1.0, size(), scene.camera, &mut anchors);
    for name in [
        "place.rooms.room_a",
        "spot.left",
        "being.rooms.first",
        "being.rooms.first.face",
    ] {
        assert!(anchors.contains_key(name), "{name}");
    }
    click(&mut s, &mut v, "leave");
    assert!(Cut::step(&s.world, 1.0).unwrap().reverse);
    for _ in 0..29 {
        s.step(&[]).unwrap();
    }
    let arrived = travel::arrival(&s.world, false).unwrap();
    s.step(&[arrived]).unwrap();
    assert!(travel::arrival(&s.world, true).is_none());
    click(&mut s, &mut v, "enter_b");
    click(&mut s, &mut v, "missing widget still lands");
    assert_eq!(v.scene(&s.world, 1.0, size(), 0.0).meshes.len(), 2);
    assert!(
        !s.world.facts.iter().any(|f| f.kind == SCRIPT_FAILED),
        "{:?}",
        s.log
    );
    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    while replay.step().unwrap() {}
    assert_eq!(s.world.hash(), replay.session.world.hash());
}

#[test]
fn natural_arrival_finishes_the_fade_without_a_last_tick_jump() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    for widget in ["enter_a", "leave"] {
        click(&mut s, &mut v, widget);
        assert_eq!(Cut::step(&s.world, 0.0).unwrap().opacity(), 0.0);
        for _ in 0..29 {
            s.step(&[]).unwrap();
        }
        assert!(Cut::step(&s.world, 0.999).unwrap().opacity() < 0.001);
        assert_eq!(Cut::step(&s.world, 1.0).unwrap().opacity(), 0.0);
        let arrived = travel::arrival(&s.world, false).unwrap();
        s.step(&[arrived]).unwrap();
        assert!(Cut::step(&s.world, 0.0).is_none());
    }
}

#[test]
fn framing_checks_every_room_even_when_the_player_starts_elsewhere() {
    let s = Session::new(vec![module()], 1).unwrap();
    let mut bad = s.world.clone();
    let p = &mut bad
        .entities
        .get_mut("rooms.third")
        .unwrap()
        .get_mut(POSITION)
        .unwrap()
        .fields;
    p.insert("x".into(), 1000.0.into());
    let mut v = View::detached(s.modules(), 1);
    assert!(
        v.check_rooms(&bad, size())
            .iter()
            .any(|f| f.contains("rooms.third") && f.contains("outside"))
    );
    let mut empty = s.world.clone();
    empty
        .entities
        .get_mut("rooms.station_left")
        .unwrap()
        .get_mut(galaxy_api::engine::SPOT)
        .unwrap()
        .fields
        .insert("x".into(), 1000.0.into());
    assert!(
        v.check_rooms(&empty, size())
            .iter()
            .any(|f| f.contains("rooms.station_left") && f.contains("outside"))
    );
}

#[test]
fn a_room_panel_cannot_cover_a_being() {
    let mut module = module();
    let code = module.files.0.get_mut("scripts/main.luau").unwrap();
    *code = code.replace(
        "x = 940, y = 40, width = 280",
        "x = 200, y = 40, width = 700, height = 650",
    );
    let s = Session::new(vec![module], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    assert!(
        v.check_rooms(&s.world, size())
            .iter()
            .any(|f| f.contains("covered by"))
    );
}

#[test]
fn fade_out_restores_the_departing_player_and_never_interpolates_between_rooms() {
    let mut s = Session::new(vec![module()], 1).unwrap();
    let model = s.world.entities["rooms.first"][galaxy_api::engine::MODEL].clone();
    s.world
        .entities
        .get_mut("rooms.player")
        .unwrap()
        .insert(galaxy_api::engine::MODEL.into(), model);
    let mut v = View::detached(s.modules(), 1);
    let before = v.scene(&s.world, 1.0, size(), 0.0);
    click(&mut s, &mut v, "enter_a");
    let leaving = v.scene(&s.world, 0.5, size(), 0.0);
    assert_eq!(leaving.camera, before.camera);
    assert_eq!(leaving.meshes.len(), before.meshes.len());
    let snapshot = galaxy_view::rooms::screen_world(&s.world, Some("rooms.station"));
    assert_eq!(
        snapshot.component("rooms.player", POSITION).unwrap().fields["x"],
        -3.0
    );
    assert_eq!(
        snapshot
            .component("rooms.player", galaxy_api::engine::LOCATION)
            .unwrap()
            .fields["place"],
        "rooms.station"
    );
    assert_eq!(
        s.world
            .component("rooms.player", galaxy_api::engine::LOCATION)
            .unwrap()
            .fields["place"],
        "rooms.room_a"
    );
}

#[test]
fn a_room_reports_missing_markers_and_raised_spots_without_floors() {
    for (from, to, expected) in [
        ("spot.left", "not_a_spot", "lacks node"),
        ("camera", "not_a_camera", "camera node"),
    ] {
        let mut m = module();
        let text = m.files.0.get_mut("models/room_a.gltf").unwrap();
        *text = text.replace(from, to);
        assert!(Session::new(vec![m], 1).err().unwrap().contains(expected));
    }
    let mut m = module();
    let text = m.files.0.get_mut("models/room_a.gltf").unwrap();
    let mut model: serde_json::Value = serde_json::from_str(text).unwrap();
    for node in model["nodes"].as_array_mut().unwrap() {
        if node["name"] == "spot.left" {
            node["translation"][1] = 2.0.into();
        }
    }
    *text = serde_json::to_string(&model).unwrap();
    assert!(
        Session::new(vec![m], 1)
            .err()
            .unwrap()
            .contains("not on its floor")
    );
}

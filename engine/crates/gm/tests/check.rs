//! `galaxy-gm check` and `types` through the real executable.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn gm(cwd: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .args(args)
        .current_dir(cwd)
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or(Value::Null)
}

#[test]
fn the_test_modules_pass_check() {
    for module in [
        "tests/modules/stroll",
        "tests/modules/tally",
        "tests/play/drift",
        "tests/play/screens",
        "tests/play/rich-screens/rich",
    ] {
        let (code, out) = gm(&engine(), &["check", module]);
        let out = json(&out);
        assert_eq!(code, 0, "{out}");
        assert!(module.ends_with(out["module"].as_str().unwrap()), "{out}");
    }
    let (_, out) = gm(&engine(), &["check", "tests/modules/stroll"]);
    assert_eq!(json(&out)["context_modules"], serde_json::json!(["tally"]));
}

#[test]
fn check_reports_unused_missing_glyphs_missing_icons_and_unreadable_grids() {
    for (catalogue, widget, expected) in [
        (
            "[icons]\nunused='\u{10ffff}'",
            "ui.text{text='Readable'}",
            "font cannot draw",
        ),
        (
            "[icons]\nunused=42",
            "ui.text{text='Readable'}",
            "glyph string",
        ),
        ("[icons]", "ui.icon{icon='absent'}", "missing module icon"),
        (
            "[icons]",
            "ui.grid{width=10,columns=2,children={ui.text{text='Wide'},ui.text{text='W'}}}",
            "clipped",
        ),
        (
            "[icons]",
            "ui.drop_target{id='target',on='drop',text='',accepts={small=''}}",
            "empty text",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("example");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("manifest.toml"), "id='example'\nversion='0.1.0'\nmin_engine_version='0.4.0'\nscripts=['screen.luau']\nevents=['drop']").unwrap();
        std::fs::write(dir.join("icons.toml"), catalogue).unwrap();
        std::fs::write(
            dir.join("screen.luau"),
            format!("return {{screen=function(_,ui) return {widget} end}}"),
        )
        .unwrap();
        let (code, out) = gm(tmp.path(), &["check", "example"]);
        assert_eq!(code, 5, "{out}");
        assert!(
            messages(&json(&out)).iter().any(|m| m.contains(expected)),
            "{out}"
        );
    }
}

/// A module folder `dir/<id>` with these files beside its manifest.
fn write_module(dir: &Path, id: &str, writes: &str, files: &[(&str, &str)]) {
    let root = dir.join(id);
    std::fs::create_dir_all(&root).unwrap();
    let manifest = format!(
        "id = \"{id}\"\nversion = \"0.1.0\"\nmin_engine_version = \"{}\"\nwrites = [{writes}]\n",
        env!("CARGO_PKG_VERSION")
    );
    std::fs::write(root.join("manifest.toml"), manifest).unwrap();
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn messages(out: &Value) -> Vec<String> {
    out["findings"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|f| f["message"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn missing_models_and_looks_and_broken_models_are_findings() {
    let tmp = tempfile::tempdir().unwrap();
    write_module(
        tmp.path(),
        "shapes",
        r#""engine.position", "engine.model""#,
        &[
            (
                "start.toml",
                "[entities.a]\n\"engine.position\" = {}\n\
                 \"engine.model\" = { model = \"shapes.cube\", look = \"shapes.shiny\" }\n",
            ),
            ("models/broken.gltf", "{"),
            ("style.toml", "[looks.dull]\nhatch = 0.5\n"),
        ],
    );
    let (code, out) = gm(tmp.path(), &["check", "shapes"]);
    let out = json(&out);
    assert_eq!(code, 5, "{out}");
    let messages = messages(&out);
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages[0].contains("models/broken.gltf"), "{messages:?}");
    assert!(
        messages[1].contains("the model `shapes.cube`"),
        "{messages:?}"
    );
    assert!(
        messages[2].contains("the look `shapes.shiny`"),
        "{messages:?}"
    );
    assert!(messages.iter().all(|m| !m.contains("  ")), "{messages:?}");
}

#[test]
fn a_shared_style_conflict_is_found_on_the_modules_in_it_only() {
    let tmp = tempfile::tempdir().unwrap();
    write_module(tmp.path(), "aa", "", &[]);
    write_module(
        tmp.path(),
        "aa-one",
        "",
        &[("style.toml", "[sky]\nstars = 1\n")],
    );
    write_module(
        tmp.path(),
        "aa-two",
        "",
        &[("style.toml", "[sky]\nstars = 2\n")],
    );
    let (code, out) = gm(tmp.path(), &["check", "aa"]);
    assert_eq!(code, 0, "aa sets no sky: {out}");
    for id in ["aa-two", "aa-one"] {
        let (code, out) = gm(tmp.path(), &["check", id]);
        let out = json(&out);
        assert_eq!(code, 5, "{out}");
        assert_eq!(
            messages(&out),
            [
                "[sky] is set by aa-one, aa-two; one module sets the whole scene's sky, and the \
              last one (aa-two) is used"
            ],
        );
    }
}

#[test]
fn a_module_with_problems_is_rejected_with_findings() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("broken");
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            "id = \"broken\"\nversion = \"0.1.0\"\nmin_engine_version = \"{}\"\n\
             scripts = [\"scripts/main.luau\"]\n",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("scripts/main.luau"),
        "return { on = { nothing = function() end } }",
    )
    .unwrap();
    let (code, out) = gm(tmp.path(), &["check", "broken"]);
    let out = json(&out);
    assert_eq!((code, out["kind"].as_str()), (5, Some("findings")), "{out}");
    assert_eq!(out["findings"][0]["kind"], "script");
}

#[test]
fn types_print() {
    let (code, types) = gm(&engine(), &["types"]);
    assert_eq!(code, 0);
    assert!(types.contains("export type Ctx"), "{types}");
}

#[test]
fn screen_check_is_headless_and_reports_widget_paths() {
    let (code, out) = gm(&engine(), &["check", "tests/play/screens-unreadable"]);
    assert_eq!(code, 5, "{out}");
    let report = json(&out);
    let findings = report["findings"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|p| p["kind"] == "screen" && p["message"].as_str().unwrap().contains("/tiny[1]"))
    );
    assert!(
        findings
            .iter()
            .any(|p| p["message"].as_str().unwrap().contains("/outside[2]"))
    );
}

#[test]
fn life_sign_overlap_only_mid_walk_fails_check_and_passes_when_fixed() {
    use galaxy_api::{Batch, Cause, Command};
    use galaxy_module::module::{Module, ModuleFiles};
    use galaxy_render::glam::Vec2;
    use galaxy_sim::Session;
    use galaxy_view::View;

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("life");
    let files = ModuleFiles::read(&engine().join("tests/play/life/demo")).unwrap();
    for (path, text) in files.0 {
        let text = if path == "start.toml" {
            text.split("[entities.speech]").next().unwrap().to_owned()
        } else {
            text
        };
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let script = dir.join("scripts/main.luau");
    let panel = |x| {
        format!(
            "return {{ screen = function(_, ui) return ui.panel {{ id='panel', x={x}, y=210, width=40, height=20, pad=0 }} end }}"
        )
    };
    std::fs::write(&script, panel(450)).unwrap();
    let mut s = Session::new(vec![Module::read(&dir).unwrap()], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    let size = Vec2::new(1280.0, 720.0);
    v.scene(&s.world, 1.0, size, 0.0);
    assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
    s.world
        .apply(
            &Batch {
                cause: Cause::Engine {
                    service: "test".into(),
                },
                commands: vec![Command::AdvanceClock {
                    entity: "life.clock".into(),
                    amount: 1,
                }],
            },
            &s.registry().clone(),
        )
        .unwrap();
    // Both endpoints are clear; only the silent sign crosses the panel mid-walk.
    let mut live_overlap = false;
    for _ in 0..160 {
        v.before_step(&s.world);
        s.step(&[]).unwrap();
        v.scene(&s.world, 0.5, size, 0.0);
        live_overlap |= v
            .screens
            .problems
            .iter()
            .any(|p| p.contains("life.walker/sign: overlaps"));
    }
    assert!(live_overlap, "expected a live mid-walk sign overlap");
    assert!(
        v.screens.problems.is_empty(),
        "endpoint: {:?}",
        v.screens.problems
    );
    let (code, out) = gm(tmp.path(), &["check", "life"]);
    assert_eq!(code, 5, "{out}");
    assert!(
        messages(&json(&out))
            .iter()
            .any(|m| m.contains("life transition")
                && m.contains("life.walker/sign: overlaps life:scripts/main.luau/screen")),
        "{out}"
    );
    std::fs::write(&script, panel(940)).unwrap();
    let (code, out) = gm(tmp.path(), &["check", "life"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn check_passes_a_resting_button_and_lists_its_reason_as_its_hover() {
    let (code, out) = gm(&engine(), &["check", "tests/play/resting/demo"]);
    assert_eq!(code, 0, "{out}");
    let report = json(&out);
    // Start, one in the tray and a full tray: Add rests there, and nothing presses Empty
    // while it rests.
    assert_eq!(report["pressed"][0]["states"], 3, "{report}");
    let empty = report["hovers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["widget"] == "demo:scripts/screen.luau/screen/row[2]/empty[2]")
        .expect("the resting button is hovered");
    assert_eq!(empty["hover"]["text"], "Nothing to empty yet.", "{report}");
    assert_eq!(empty["hover"]["readable"], true, "{report}");
    // A style whose resting look can't be told from a ready button is refused.
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("demo");
    copy(&engine().join("tests/play/resting/demo"), &module);
    std::fs::write(
        module.join("style.toml"),
        "[screen]\nbutton = '#dddddd'\nresting = '#dddddd'\nresting_dash = 0.0\n",
    )
    .unwrap();
    let (code, out) = gm(&engine(), &["check", module.to_str().unwrap()]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("resting must differ from button"), "{out}");
}

#[test]
fn check_refuses_a_resting_button_too_tightly_padded_to_show_its_frame() {
    // The frame is capped by the padding: with none, a resting button on a fill like
    // `button`'s would look ready.
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("demo");
    copy(&engine().join("tests/play/resting/demo"), &module);
    std::fs::write(module.join("style.toml"), "[screen]\nresting = '#dddddd'\n").unwrap();
    let screen = |pad: f32| {
        format!(
            "return {{
    on = {{ add = function() end, empty = function() end }},
    screen = function(_, ui)
        return ui.row {{ width = 412, children = {{
            ui.button {{ id = \"ready\", text = \"Go\", on = \"add\",
                width = 200, height = 60, pad = {pad} }},
            ui.button {{ id = \"rest\", text = \"Go\", on = \"empty\",
                width = 200, height = 60, pad = {pad}, resting = \"Not yet.\" }},
        }} }}
    end,
}}
"
        )
    };
    let script = module.join("scripts/screen.luau");
    std::fs::write(&script, screen(0.0)).unwrap();
    let (code, out) = gm(&engine(), &["check", module.to_str().unwrap()]);
    assert_eq!(code, 5, "{out}");
    assert!(
        messages(&json(&out))
            .iter()
            .any(|m| m.contains("rest[2]")
                && m.contains("resting button needs pad of at least 1 pixel")),
        "{out}"
    );
    std::fs::write(&script, screen(1.0)).unwrap();
    let (code, out) = gm(&engine(), &["check", module.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn check_presses_a_button_to_find_a_panel_over_a_name_sign() {
    let (code, out) = gm(&engine(), &["check", "tests/play/sign-panel/demo"]);
    assert_eq!(code, 5, "{out}");
    let report = json(&out);
    let findings = report["findings"].as_array().unwrap();
    assert!(
        findings.iter().all(|p| p["kind"] == "press"),
        "only a press reaches it: {report}"
    );
    assert!(findings.iter().any(|p| {
        let m = p["message"].as_str().unwrap();
        m.contains("after pressing `toggle`") && m.contains("signpanel.host/sign: overlaps")
    }));
    assert_eq!(report["pressed"][0]["states"], 2, "{report}");
    // The offer's answers panel opens clear of its sign, and its choice bubble lays out.
    let (code, out) = gm(&engine(), &["check", "tests/play/offer/demo"]);
    assert_eq!(code, 0, "{out}");
    let report = json(&out);
    assert_eq!(report["pressed"][0]["states"], 2, "{report}");
    assert!(
        report["hovers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["widget"] == "offer.gift/talk/choices/offer.take"),
        "{report}"
    );
}

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
fn missing_models_and_looks_and_broken_models_are_findings() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("shapes");
    std::fs::create_dir_all(dir.join("models")).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            "id = \"shapes\"
version = \"0.1.0\"
min_engine_version = \"{}\"
             writes = [\"engine.position\", \"engine.model\"]
",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("start.toml"),
        "[entities.a]
\"engine.position\" = {}
         \"engine.model\" = { model = \"shapes.cube\", look = \"shapes.shiny\" }
",
    )
    .unwrap();
    std::fs::write(dir.join("models/broken.gltf"), "{").unwrap();
    std::fs::write(
        dir.join("style.toml"),
        "[looks.dull]
hatch = 0.5
",
    )
    .unwrap();
    let (code, out) = gm(tmp.path(), &["check", "shapes"]);
    let out = json(&out);
    assert_eq!(code, 5, "{out}");
    let messages: Vec<&str> = out["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["message"].as_str().unwrap())
        .collect();
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

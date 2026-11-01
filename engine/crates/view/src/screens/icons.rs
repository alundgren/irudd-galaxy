//! Module-owned glyph names. Text assets travel with saves and recordings.
use super::*;

pub(super) fn load(
    modules: &[&Module],
    measure: &mut TextMeasure,
) -> (BTreeMap<String, String>, Vec<String>) {
    let mut icons = BTreeMap::new();
    let mut problems = Vec::new();
    for module in modules {
        let Some(text) = module.files.get("icons.toml") else {
            continue;
        };
        let table = match text.parse::<toml::Table>() {
            Ok(table) => table,
            Err(e) => {
                problems.push(format!("{}:icons.toml: {e}", module.id()));
                continue;
            }
        };
        let Some(table) = table.get("icons").and_then(toml::Value::as_table) else {
            problems.push(format!(
                "{}:icons.toml: needs an [icons] table",
                module.id()
            ));
            continue;
        };
        for (name, value) in table {
            let id = format!("{}.{}", module.id(), name);
            let Some(glyph) = value.as_str().filter(|s| !s.trim().is_empty()) else {
                problems.push(format!("{id}: icon must be a nonempty glyph string"));
                continue;
            };
            if !measure
                .missing(&Text::new(glyph, Vec2::ZERO, 20.0), 1.0)
                .is_empty()
            {
                problems.push(format!(
                    "{id}: icon has glyphs the bundled font cannot draw"
                ));
            }
            icons.insert(id, glyph.into());
        }
    }
    (icons, problems)
}

pub(super) fn resolve(
    w: &mut Widget,
    icons: &BTreeMap<String, String>,
    problems: &mut Vec<String>,
) {
    if w.kind == "icon" {
        let module = w.path.split(':').next().unwrap_or_default();
        let name = format!("{module}.{}", w.value);
        match icons.get(&name) {
            Some(glyph) => w.text = glyph.clone(),
            None => {
                problems.push(format!("{}: missing module icon `{}`", w.path, w.value));
                w.text = "?".into();
            }
        }
    }
    for child in &mut w.children {
        resolve(child, icons, problems);
    }
}

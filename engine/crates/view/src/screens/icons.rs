//! Module icons: glyph strings, or small SVG files drawn in the text colour (or in their
//! own paint). Both travel with the module's text files in saves and recordings.
use std::sync::Arc;

use galaxy_render::vector::Icon;

use super::*;

#[derive(Clone, Debug)]
pub(crate) enum Asset {
    Glyph(String),
    /// Drawn as a mask in the text colour of what it sits in, unless `painted`.
    Vector {
        icon: Arc<Icon>,
        painted: bool,
    },
}

pub(crate) type Icons = BTreeMap<String, Asset>;

pub(super) fn load(modules: &[&Module], measure: &mut TextMeasure) -> (Icons, Vec<String>) {
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
            let path = format!("{}:icons.toml/{name}", module.id());
            match asset(module, value, measure) {
                Ok(asset) => {
                    icons.insert(id, asset);
                }
                Err(e) => problems.push(format!("{path}: {e}")),
            }
        }
    }
    (icons, problems)
}

fn asset(module: &Module, value: &toml::Value, measure: &mut TextMeasure) -> Result<Asset, String> {
    if let Some(glyph) = value.as_str() {
        if glyph.trim().is_empty() {
            return Err("icon must be a nonempty glyph string".into());
        }
        if !measure
            .missing(&Text::new(glyph, Vec2::ZERO, 20.0), 1.0)
            .is_empty()
        {
            return Err("icon has glyphs the bundled font cannot draw".into());
        }
        return Ok(Asset::Glyph(glyph.into()));
    }
    let Some(table) = value.as_table() else {
        return Err("an icon is a glyph string or { svg = \"icons/<name>.svg\" }".into());
    };
    if let Some(key) = table.keys().find(|k| *k != "svg" && *k != "painted") {
        return Err(format!("unknown key `{key}`: an icon has svg and painted"));
    }
    let file = table
        .get("svg")
        .and_then(toml::Value::as_str)
        .ok_or("an image icon needs svg = \"<file>.svg\" inside the module")?;
    let painted = match table.get("painted") {
        None => false,
        Some(v) => v.as_bool().ok_or("painted is true or false")?,
    };
    if !file.ends_with(".svg") {
        return Err(format!("`{file}` is not an .svg file"));
    }
    let text = module
        .files
        .get(file)
        .ok_or_else(|| format!("`{file}` is not in the module"))?;
    let icon = Icon::parse(text).map_err(|e| format!("{file}: {e}"))?;
    Ok(Asset::Vector {
        icon: Arc::new(icon),
        painted,
    })
}

pub(super) fn resolve(w: &mut Widget, icons: &Icons, problems: &mut Vec<String>) {
    let module = w.path.split(':').next().unwrap_or_default().to_string();
    let id = |name: &str| format!("{module}.{name}");
    if w.kind == "icon" {
        match icons.get(&id(&w.value)) {
            Some(Asset::Glyph(glyph)) => w.text = glyph.clone(),
            Some(Asset::Vector { .. }) => {
                w.value = id(&w.value);
                w.icon = Some(w.value.clone());
            }
            None => {
                problems.push(format!("{}: missing module icon `{}`", w.path, w.value));
                w.text = "?".into();
            }
        }
    } else if let Some(name) = w.icon.take() {
        match icons.get(&id(&name)) {
            // A glyph is part of the label: put it in front, in the label's own look.
            Some(Asset::Glyph(glyph)) => {
                let lead = format!("{glyph} ");
                if !w.spans.is_empty() {
                    w.spans.insert(
                        0,
                        galaxy_script::screens::Span {
                            text: lead.clone(),
                            look: None,
                        },
                    );
                }
                w.text = format!("{lead}{}", w.text);
                w.icon_at = None;
            }
            Some(Asset::Vector { .. }) => w.icon = Some(id(&name)),
            None => problems.push(format!("{}: missing module icon `{name}`", w.path)),
        }
    }
    for child in &mut w.children {
        resolve(child, icons, problems);
    }
}

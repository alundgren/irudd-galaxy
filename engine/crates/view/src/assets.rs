//! What modules bring for the eye: models (`models/<name>.gltf`) and styles
//! (`style.toml`), read from the module files a session runs.

use std::collections::BTreeMap;

use galaxy_mesh::MeshData;
use galaxy_module::module::Module;
use galaxy_render::style::{Ink, Light, Look, Sky, Style};

/// The folder models live in, inside a module.
pub const MODELS: &str = "models/";
/// The style file in a module.
pub const STYLE: &str = "style.toml";

/// The models and looks of a set of modules, and the scene-wide style.
#[derive(Clone, Debug, Default)]
pub struct Assets {
    /// Models by ID (`<module>.<name>`).
    pub models: BTreeMap<String, MeshData>,
    /// Looks by ID (`<module>.<name>`).
    pub looks: BTreeMap<String, Look>,
    pub ink: Ink,
    pub light: Light,
    pub sky: Sky,
    /// What could not be read, each with its module and file.
    pub problems: Vec<String>,
}

impl Assets {
    /// Reads every module's models and style. A file that doesn't parse is left out and
    /// listed in [`Assets::problems`]; the rest still loads.
    pub fn load<'a>(modules: impl IntoIterator<Item = &'a Module>) -> Assets {
        let mut out = Assets::default();
        let mut scene_wide: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for module in modules {
            let id = module.id();
            for (path, text) in &module.files.0 {
                let Some(name) = path
                    .strip_prefix(MODELS)
                    .and_then(|p| p.strip_suffix(".gltf"))
                else {
                    continue;
                };
                match galaxy_mesh::gltf::read(text) {
                    Ok(mesh) => {
                        out.models.insert(format!("{id}.{name}"), mesh);
                    }
                    Err(e) => out.problems.push(format!("{id}: {path}: {e}")),
                }
            }
            let Some(text) = module.files.get(STYLE) else {
                continue;
            };
            let style = match Style::parse(text) {
                Ok(style) => style,
                Err(e) => {
                    out.problems.push(format!("{id}: {STYLE}: {e}"));
                    continue;
                }
            };
            if let Some(ink) = style.ink {
                out.ink = ink;
                scene_wide.entry("ink").or_default().push(id.to_owned());
            }
            if let Some(light) = style.light {
                out.light = light;
                scene_wide.entry("light").or_default().push(id.to_owned());
            }
            if let Some(sky) = style.sky {
                out.sky = sky;
                scene_wide.entry("sky").or_default().push(id.to_owned());
            }
            for (name, look) in style.looks {
                out.looks.insert(format!("{id}.{name}"), look);
            }
        }
        for (table, ids) in scene_wide {
            if ids.len() > 1 {
                out.problems.push(format!(
                    "[{table}] is set by {}; one module sets the whole scene's {table}, and \
                     the last one ({}) is used",
                    ids.join(", "),
                    ids.last().expect("more than one")
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use galaxy_mesh::gltf::{Part, write};
    use galaxy_mesh::{Color, MeshBuilder};
    use galaxy_render::glam::Vec3;

    fn module(id: &str, files: &[(&str, &str)]) -> Module {
        let dir = tempfile::tempdir().unwrap();
        let manifest =
            format!("id = \"{id}\"\nversion = \"0.1.0\"\nmin_engine_version = \"0.1.0\"\n");
        std::fs::write(dir.path().join("manifest.toml"), manifest).unwrap();
        for (path, text) in files {
            let p = dir.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        Module::read(dir.path()).unwrap()
    }

    fn cube() -> String {
        let mut b = MeshBuilder::new();
        b.paint(Color::hex(0xd9774b), 0.0);
        b.cuboid(Vec3::ONE);
        let mesh = b.build();
        write(&[Part {
            name: "cube",
            mesh: &mesh,
        }])
    }

    #[test]
    fn models_and_looks_are_named_by_module() {
        let cube = cube();
        let a = module(
            "aa",
            &[
                ("models/cube.gltf", &cube),
                (
                    "style.toml",
                    "[looks.hull]\noutline_px = 5.0\n[ink]\ninner_px = 3.0\n",
                ),
            ],
        );
        let assets = Assets::load([&a]);
        assert!(assets.problems.is_empty(), "{:?}", assets.problems);
        assert!(assets.models.contains_key("aa.cube"));
        assert_eq!(assets.looks["aa.hull"].outline_px, 5.0);
        assert_eq!(assets.ink.inner_px, 3.0);
    }

    #[test]
    fn problems_name_the_module_and_file_and_the_rest_loads() {
        let cube = cube();
        let a = module(
            "aa",
            &[
                ("models/cube.gltf", &cube),
                ("models/broken.gltf", "{"),
                ("style.toml", "[sky]\nstars = 3\n"),
            ],
        );
        let b = module(
            "bb",
            &[("style.toml", "[sky]\nstars = 4\n[looks.x]\nbands = 9\n")],
        );
        let assets = Assets::load([&a, &b]);
        assert!(assets.models.contains_key("aa.cube"));
        let all = assets.problems.join("\n");
        assert!(all.contains("aa: models/broken.gltf"), "{all}");
        assert!(all.contains("bb: style.toml: [looks.x]"), "{all}");
        assert_eq!(assets.sky.stars, 3, "bb's style failed, so aa's sky stands");
    }

    #[test]
    fn two_modules_setting_the_sky_is_a_problem() {
        let a = module("aa", &[("style.toml", "[sky]\nstars = 3\n")]);
        let b = module("bb", &[("style.toml", "[sky]\nstars = 4\n")]);
        let assets = Assets::load([&a, &b]);
        assert_eq!(assets.sky.stars, 4);
        assert!(assets.problems[0].contains("[sky] is set by aa, bb"));
    }
}

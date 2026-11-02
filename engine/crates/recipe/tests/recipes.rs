//! The model generator against the yard test module and against bad recipes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use galaxy_mesh::gltf::{read_nodes, read_parts};
use galaxy_module::module::ModuleFiles;
use galaxy_recipe::params::Kind;
use galaxy_recipe::shapes::SHAPES;
use galaxy_recipe::{make, names};

fn yard() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/yard")
}

fn yard_files() -> BTreeMap<String, String> {
    ModuleFiles::read(&yard()).expect("the yard reads").0
}

/// The SHA-256 of each yard model as made from its recipe. The same on every OS: this
/// test runs on Windows and in `cargo xtask linux-check`, so a difference in floating
/// point between platforms fails one of them. Change these only on purpose (a shape or
/// a recipe changed), remake with `galaxy-gm model make tests/play/yard`, and say so in
/// the PR.
const HASHES: &[(&str, &str)] = &[
    (
        "outpost",
        "8b15b5008e6654336f2744d439089ec141e98559b3e4948547317fa163f8c60c",
    ),
    (
        "rock",
        "247d92cfeedb616c8e3ab282bce6e0b8fe10fbcf7df67af362acd2829ad26afc",
    ),
    (
        "rock_pale",
        "da400f5cd86968fd7498fb5e46e2f21f28b7bc20a5497ef89272a5c7605a93a6",
    ),
    (
        "skiff",
        "d99a290ce8f458b92611bb94631d89993551b1284a7514f56b02d23f9c8c65c6",
    ),
    (
        "thruster",
        "ed392968aa83648e817b9cb97ffbdc9c555de743ea1f201d55a4842c7d4ab708",
    ),
];

#[test]
fn yard_models_are_what_their_recipes_make_with_the_same_bytes_everywhere() {
    let files = yard_files();
    let recipes = names(&files);
    let pinned: Vec<&str> = HASHES.iter().map(|h| h.0).collect();
    assert_eq!(recipes, pinned, "every yard recipe has a pinned hash");
    let mut wrong = Vec::new();
    for (name, hash) in HASHES {
        let made = make(&files, name).unwrap_or_else(|p| panic!("{p}"));
        assert_eq!(
            files.get(&made.file).map(String::as_str),
            Some(made.text.as_str()),
            "{} is not what {} makes; run galaxy-gm model make tests/play/yard",
            made.file,
            made.recipe
        );
        if made.sha256 != *hash {
            wrong.push(format!("(\"{name}\", \"{}\")", made.sha256));
        }
        // Made twice, the same bytes.
        assert_eq!(make(&files, name).unwrap().text, made.text);
    }
    assert!(wrong.is_empty(), "hashes differ: {}", wrong.join(", "));
}

#[test]
fn yard_models_read_as_the_parts_and_markers_their_recipes_name() {
    let files = yard_files();
    let outpost = make(&files, "outpost").unwrap();
    let (mesh, parts) = read_parts(&outpost.text).unwrap();
    assert!(mesh.is_well_formed());
    let read: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
    let mut want: Vec<&str> = outpost.parts.iter().map(String::as_str).collect();
    let mut got = read.clone();
    want.sort_unstable();
    got.sort_unstable();
    assert_eq!(got, want);
    let nodes = read_nodes(&outpost.text).unwrap();
    let dock = &nodes["spot.dock"];
    assert!((dock.position.y - 11.5).abs() < 1e-4, "{:?}", dock.position);
    assert!(dock.forward.y < -0.99, "the dock spot faces the centre");
    let camera = &nodes["camera"];
    assert!(
        camera.forward.x < -0.8 && camera.forward.z < -0.4,
        "{camera:?}"
    );
    assert!(camera.up.z > 0.8);

    // The rock morphs and its cuts carry a bite; the pale one is the same rock repainted.
    let rock = make(&files, "rock").unwrap();
    let (mesh, _) = read_parts(&rock.text).unwrap();
    let moved = mesh.vertices.iter().any(|v| v.morph_position != v.position);
    assert!(moved, "the rock has a morph target");
    assert!(mesh.vertices.iter().any(|v| v.bite[1] > 0.5));
    let pale = make(&files, "rock_pale").unwrap();
    let (pale_mesh, _) = read_parts(&pale.text).unwrap();
    assert_eq!(pale_mesh.triangle_count(), mesh.triangle_count());
    assert_ne!(pale_mesh.vertices[0].color, mesh.vertices[0].color);

    // The skiff's two thrusters are one part, both sides.
    let skiff = make(&files, "skiff").unwrap();
    let (mesh, parts) = read_parts(&skiff.text).unwrap();
    let thrusters = parts.iter().find(|p| p.name == "thrusters").unwrap();
    let ys: Vec<f32> = mesh.vertices[thrusters.vertices.clone()]
        .iter()
        .map(|v| v.position[1])
        .collect();
    assert!(ys.iter().any(|y| *y > 0.3) && ys.iter().any(|y| *y < -0.3));
    let glow = mesh.vertices[thrusters.vertices.clone()]
        .iter()
        .any(|v| v.color[3] > 0.9);
    assert!(glow, "the used recipe's lights glow");
}

/// A module of one palette and these recipes.
fn module(recipes: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    files.insert(
        "recipes/palette.toml".to_owned(),
        "main = \"#efdcb4\"\ntrim = \"#d9774b\"\naccent = \"#3fa7a0\"\nlights = \"#ffcf6b\"\nglass = \"#9fe0c8\"\n".to_owned(),
    );
    for (name, text) in recipes {
        files.insert(format!("recipes/{name}.toml"), (*text).to_owned());
    }
    files
}

fn refused(text: &str) -> galaxy_recipe::Problem {
    let files = module(&[("a", text), ("b", "[[part]]\nuse = \"a\"\n")]);
    match make(&files, "a") {
        Ok(_) => panic!("made a bad recipe:\n{text}"),
        Err(p) => p,
    }
}

#[test]
fn a_bad_recipe_is_refused_naming_its_field() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "[[part]]\nshape = \"tiers\"\nradius = -1.0\nheight = 2.0\n",
            "part[0].radius",
            "expected 0.5 to 60",
        ),
        (
            "[[part]]\nshape = \"tiers\"\nheight = 2.0\n",
            "part[0].radius",
            "missing",
        ),
        (
            "[[part]]\nshape = \"tiers\"\nradius = 2.0\nheight = 2.0\nradus = 1.0\n",
            "part[0].radus",
            "not a key",
        ),
        (
            "[[part]]\nshape = \"tower\"\n",
            "part[0].shape",
            "not a shape",
        ),
        ("[[part]]\nradius = 1.0\n", "part[0].shape", "missing"),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1]\n",
            "part[0].size",
            "[x, y, z]",
        ),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\npaint = \"rust\"\n",
            "part[0].paint",
            "slot `rust`",
        ),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\npaint = \"#12345\"\n",
            "part[0].paint",
            "#rrggbb",
        ),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\nlights = \"lights\"\n",
            "part[0].lights",
            "not a paint box uses",
        ),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\ncopies = 99\n",
            "part[0].copies",
            "1 to 32",
        ),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\nat = [0, 0, 900]\n",
            "part[0].at",
            "-500 to 500",
        ),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\nname = \"Big Box\"\n",
            "part[0].name",
            "not a valid name",
        ),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\nname = \"x\"\n[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\nname = \"x\"\n",
            "part[1].name",
            "earlier part",
        ),
        (
            "[[part]]\nshape = \"spoked_ring\"\nradius = 4.0\ntube = 3.0\n",
            "part[0].tube",
            "half the radius",
        ),
        (
            "[[part]]\nshape = \"lathe\"\nprofile = [[1, 0], [-1, 1]]\n",
            "part[0].profile",
            "negative radius",
        ),
        (
            "[[part]]\nshape = \"plate\"\noutline = [[0, 0], [1, 0], [2, 0]]\nheight = 1\n",
            "part[0].outline",
            "no area",
        ),
        ("[[part]]\nuse = \"b\"\n", "part[0].use", "goes round"),
        (
            "[[part]]\nuse = \"nope\"\n",
            "part[0].use",
            "not a recipe here",
        ),
        (
            "[[part]]\nuse = \"b\"\npaint = \"main\"\n",
            "part[0].paint",
            "not taken with use",
        ),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\npalette = { main = \"#ffffff\" }\n",
            "part[0].palette",
            "use only",
        ),
        (
            "seed = -3\n[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\n",
            "seed",
            "0 or more",
        ),
        (
            "colour = 3\n[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\n",
            "colour",
            "not a recipe key",
        ),
        (
            "[palette]\nmain = \"red\"\n[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\n",
            "palette.main",
            "#rrggbb",
        ),
        ("about = \"nothing\"\n", "part", "at least one"),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\n[[marker]]\nat = [0, 0, 0]\n",
            "marker[0].name",
            "missing",
        ),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\n[[marker]]\nname = \"box\"\n",
            "marker[0].name",
            "a part",
        ),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\n[[marker]]\nname = \"camera\"\npitch = 95\n",
            "marker[0].pitch",
            "-89 to 89",
        ),
        (
            "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\n[[marker]]\nname = \"camera\"\nscale = 2\n",
            "marker[0].scale",
            "not a marker key",
        ),
        ("[[part]\nshape = \"box\"\n", "", "not valid TOML"),
    ];
    for (text, field, says) in cases {
        let p = refused(text);
        assert_eq!(p.field, *field, "{text}\n{p}");
        assert!(p.message.contains(says), "{text}\n{p}");
        // A cycle is found in the recipe that closes it.
        let file = if *says == "goes round" {
            "recipes/b.toml"
        } else {
            "recipes/a.toml"
        };
        assert_eq!(p.file, file, "{p}");
        assert!(p.to_string().contains(field), "{p}");
    }
    // A bad palette file is named too.
    let mut files = module(&[("a", "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\n")]);
    files.insert("recipes/palette.toml".into(), "Main = \"#ffffff\"\n".into());
    let p = make(&files, "a").unwrap_err();
    assert_eq!(
        (p.file.as_str(), p.field.as_str()),
        ("recipes/palette.toml", "Main")
    );
}

/// The value a key gets at `at` of the way through its range (required keys only).
fn sample(kind: Kind, at: f32) -> Option<String> {
    let mix = |lo: f32, hi: f32| lo + (hi - lo) * at;
    match kind {
        Kind::Num(lo, hi, _) => Some(format!("{:?}", mix(lo, hi))),
        Kind::Int(lo, hi, _) => Some(format!("{}", mix(lo as f32, hi as f32).round() as u32)),
        Kind::Flag(_) => Some(format!("{}", at > 0.5)),
        Kind::Vec3(lo, hi, _) => {
            let v = mix(lo.max(-5.0), hi.min(5.0));
            Some(format!("[{v:?}, {:?}, 0.5]", v * 0.5 + 1.0))
        }
        Kind::Pairs(_, _) => None,
    }
}

#[test]
fn every_shape_builds_well_formed_across_its_ranges_and_refuses_past_them() {
    for shape in SHAPES {
        // Shapes with relational limits get values well inside them.
        for at in [0.0f32, 0.15, 0.5, 1.0] {
            let mut text = format!("[[part]]\nshape = \"{}\"\n", shape.name);
            for p in shape.params {
                let value = match (shape.name, p.key, p.kind) {
                    (_, "profile", _) => "[[0.0, 0.0], [1.0, 0.5], [0.6, 1.2], [0.0, 1.5]]".into(),
                    (_, "outline", _) => "[[0, 0], [2, 0], [2, 1], [0, 1]]".into(),
                    ("strut", "to", _) => "[1.0, 2.0, 3.0]".into(),
                    ("spoked_ring" | "cabins", "tube", Kind::Num(lo, _, _)) => format!("{lo:?}"),
                    ("spoked_ring", "inner", _) => "0.5".into(),
                    ("ring", "tube", Kind::Num(lo, _, _)) => format!("{lo:?}"),
                    (_, _, kind) => sample(kind, at).unwrap(),
                };
                text.push_str(&format!("{} = {value}\n", p.key));
            }
            let files = module(&[("a", &text)]);
            let made = make(&files, "a").unwrap_or_else(|p| panic!("{}: {p}\n{text}", shape.name));
            let (mesh, parts) = read_parts(&made.text).unwrap();
            assert!(mesh.is_well_formed(), "{} at {at}", shape.name);
            assert_eq!(parts.len(), 1, "{}", shape.name);
            assert!(
                mesh.vertices
                    .iter()
                    .all(|v| v.position.iter().all(|c| c.is_finite()))
            );
        }
        // Past the top of a range, the key is named.
        for p in shape.params {
            let over = match p.kind {
                Kind::Num(_, hi, _) => format!("{:?}", hi * 2.0 + 1.0),
                Kind::Int(_, hi, _) => format!("{}", hi + 1),
                _ => continue,
            };
            let mut text = format!("[[part]]\nshape = \"{}\"\n{} = {over}\n", shape.name, p.key);
            for q in shape.params.iter().filter(|q| q.key != p.key) {
                if let Some(v) = sample(q.kind, 0.5) {
                    text.push_str(&format!("{} = {v}\n", q.key));
                } else {
                    text.push_str(&format!("{} = [[0, 0], [1, 0], [1, 1]]\n", q.key));
                }
            }
            let p2 = refused(&text);
            assert_eq!(
                p2.field,
                format!("part[0].{}", p.key),
                "{}: {p2}",
                shape.name
            );
        }
    }
}

#[test]
fn a_seed_varies_a_rock_and_placement_moves_the_part() {
    let rock = |seed: u32| {
        let text =
            format!("seed = {seed}\n[[part]]\nshape = \"rock\"\nradius = 2.0\naccent = \"main\"\n");
        make(&module(&[("a", &text)]), "a").unwrap().text
    };
    assert_eq!(rock(4), rock(4));
    assert_ne!(rock(4), rock(5));

    let files = module(&[(
        "a",
        "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\nangle = 90.0\ndistance = 10.0\ncopies = 4\n",
    )]);
    let made = make(&files, "a").unwrap();
    let (lo, hi) = made.bounds;
    // Four boxes round the centre, ten out.
    assert!(
        (hi.x - 10.5).abs() < 1e-3 && (lo.x + 10.5).abs() < 1e-3,
        "{lo} {hi}"
    );
    assert!(
        (hi.y - 10.5).abs() < 1e-3 && (lo.y + 10.5).abs() < 1e-3,
        "{lo} {hi}"
    );
    // Unnamed parts take their shape's name, numbered after the first.
    let files = module(&[(
        "a",
        "[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\n[[part]]\nshape = \"box\"\nsize = [1, 1, 1]\nat = [3, 0, 0]\n",
    )]);
    assert_eq!(make(&files, "a").unwrap().parts, ["box", "box.2"]);
}

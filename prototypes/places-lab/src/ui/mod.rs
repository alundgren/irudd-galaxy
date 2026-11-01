//! The screen model: a Luau function returns a tree of engine-owned widgets
//! ([`tree`]); the engine lays it out with real text measuring ([`layout`]), draws it in
//! the role's look ([`style`], [`draw`]) and checks the readability rules ([`rules`]).

pub mod draw;
pub mod layout;
pub mod rules;
pub mod style;
pub mod tree;

use render::Rect;
use render::glam::Vec2;

use layout::{Ctx, Measure};
use rules::Problems;
use tree::{Kind, Node};

/// A screen read and laid out.
pub struct Built {
    pub root: Node,
    pub problems: Problems,
    /// The placed layers' rectangles.
    pub placed: Vec<Rect>,
}

impl Built {
    pub fn empty() -> Built {
        Built {
            root: Node::empty_screen(),
            problems: Problems::default(),
            placed: Vec::new(),
        }
    }
}

pub fn build(lua: &mlua::Lua, value: &mlua::Value, m: &mut dyn Measure, screen: Vec2) -> Built {
    let mut problems = Problems::default();
    let root = match tree::read(lua, value, "", &mut problems) {
        Some(n) if n.kind == Kind::Screen => n,
        Some(n) => {
            problems.add(&n.path, "view must return ui.screen{...}");
            Node::empty_screen()
        }
        None => Node::empty_screen(),
    };
    let mut root = root;
    let placed = {
        let mut cx = Ctx {
            m,
            problems: &mut problems,
        };
        layout::place_screen(&mut cx, &mut root, screen)
    };
    Built {
        root,
        problems,
        placed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character 10 px wide, lines 1.25 x the size: enough to test layout rules
    /// without a GPU.
    struct Fixed;

    impl Measure for Fixed {
        fn text(&mut self, text: &str, size: f32, _weight: u16, wrap: Option<f32>) -> Vec2 {
            let w = text.chars().count() as f32 * 10.0;
            match wrap {
                Some(max) if w > max => Vec2::new(max, (w / max).ceil() * size * 1.25),
                _ => Vec2::new(w, size * 1.25),
            }
        }

        fn missing(&mut self, text: &str) -> Vec<char> {
            text.chars().filter(|c| *c == '→').collect()
        }
    }

    fn build_src(src: &str) -> Built {
        let lua = mlua::Lua::new();
        let prelude =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/lab/prelude.luau"))
                .unwrap();
        let ui: mlua::Table = lua.load(&prelude).eval().unwrap();
        lua.globals().set("ui", ui).unwrap();
        let v: mlua::Value = lua.load(src).eval().unwrap();
        build(&lua, &v, &mut Fixed, Vec2::new(1280.0, 720.0))
    }

    fn says(b: &Built, what: &str) -> bool {
        b.problems.0.iter().any(|p| p.what.contains(what))
    }

    #[test]
    fn a_clean_screen_has_no_problems() {
        let b = build_src(
            r#"return ui.screen({ ui.panel({ role = "room", place = "right", ui.title("Market"), ui.button({ "Sell", on = "sell" }) }) })"#,
        );
        assert!(b.problems.is_empty(), "{:?}", b.problems);
    }

    #[test]
    fn a_screen_cannot_choose_a_text_colour() {
        let b = build_src(
            r#"return ui.screen({ ui.text({ "grey", color = "grey", place = "top" }) })"#,
        );
        assert!(says(&b, "has no property `color`"), "{:?}", b.problems);
    }

    #[test]
    fn a_resting_button_needs_its_reason() {
        let b = build_src(
            r#"return ui.screen({ ui.button({ "Buy", on = "buy", rest = true, place = "top" }) })"#,
        );
        assert!(says(&b, "needs its reason"), "{:?}", b.problems);
    }

    #[test]
    fn a_too_narrow_column_is_cut_off() {
        let b = build_src(
            r#"return ui.screen({ ui.row({ place = "top", ui.text({ "Refined", width = 40 }) }) })"#,
        );
        assert!(says(&b, "cut off"), "{:?}", b.problems);
    }

    #[test]
    fn a_wide_room_panel_and_overlaps_are_reported() {
        let b = build_src(
            r#"return ui.screen({
                ui.panel({ role = "room", place = "right", width = 800, ui.text("wide") }),
                ui.panel({ role = "status", place = "top-right", ui.text("1200 cr") }),
            })"#,
        );
        assert!(says(&b, "covers 62% of the width"), "{:?}", b.problems);
        assert!(says(&b, "overlaps"), "{:?}", b.problems);
    }

    #[test]
    fn a_state_colour_needs_its_word() {
        let b = build_src(
            r#"return ui.screen({ ui.tile({ "Market", icon = "market", on = "open", ribbon = { "new" }, place = "bottom" }) })"#,
        );
        assert!(says(&b, "needs its word"), "{:?}", b.problems);
    }
}

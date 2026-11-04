//! The generic screen kit (`tests/play/ui-kit`): named looks, image icons, text spans
//! and UI motion, all presentation only.
use galaxy_module::module::Module;
use galaxy_render::{Color, Overlay, Panel, Text, glam::Vec2};
use galaxy_sim::{
    Session,
    recording::{BuildStamp, Replay},
    scenario::InputsFile,
};
use galaxy_view::View;
use std::path::Path;

const SIZE: Vec2 = Vec2::new(1280.0, 720.0);

fn kit() -> Module {
    Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/ui-kit/kit"))
        .unwrap()
}

fn texts(o: &Overlay) -> Vec<&Text> {
    o.widgets.iter().flat_map(|l| &l.texts).collect()
}
fn text<'a>(o: &'a Overlay, words: &str) -> &'a Text {
    texts(o)
        .into_iter()
        .find(|t| t.text == words)
        .unwrap_or_else(|| panic!("no text `{words}`"))
}
fn panels(o: &Overlay) -> Vec<&Panel> {
    o.widgets.iter().flat_map(|l| &l.panels).collect()
}
/// The panel behind a text: the last one painted that contains its origin.
fn behind<'a>(o: &'a Overlay, t: &Text) -> &'a Panel {
    panels(o)
        .into_iter()
        .rev()
        .find(|p| {
            p.rect.x <= t.pos.x
                && t.pos.x <= p.rect.x + p.rect.w
                && p.rect.y <= t.pos.y
                && t.pos.y <= p.rect.y + p.rect.h
        })
        .unwrap()
}

#[test]
fn looks_spans_and_icons_paint_as_the_style_says() {
    let mut s = Session::new(vec![kit()], 1).unwrap();
    s.step(&[]).unwrap();
    let mut v = View::detached(s.modules(), 1);
    let o = v.scene(&s.world, 1.0, SIZE, 0.0).overlay;
    assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
    assert!(v.assets.problems.is_empty(), "{:?}", v.assets.problems);
    // A dark chip with white text beside a parchment panel with black text.
    let credits = text(&o, "Credits");
    assert_eq!(credits.color, Color::hex(0xffffff));
    let chip = behind(&o, credits);
    assert_eq!(chip.fill, Color::hex(0x1f1a24));
    assert_eq!(chip.border, Color::hex(0xf2b43c));
    assert!(chip.shadow && chip.shadow_px == 6.0);
    let title = text(&o, "Screen kit");
    assert_eq!(title.color, Color::hex(0x000000));
    assert_eq!(behind(&o, title).fill, Color::hex(0xf7e8c8));
    // The speaker's name: a bold span with an amber rule under just that word.
    let line = text(&o, "Odo has fresh ice, cheap today.");
    assert_eq!(line.spans.len(), 1);
    assert_eq!(line.spans[0].range, 0..3);
    assert_eq!(line.spans[0].weight, 700);
    let rules: Vec<_> = panels(&o)
        .into_iter()
        .filter(|p| p.fill == Color::hex(0xb07800))
        .collect();
    // One under the span, one under the bubble's heading (bubble_title_look).
    assert_eq!(rules.len(), 2, "{rules:?}");
    let rule = rules
        .iter()
        .find(|r| (r.rect.y - line.pos.y).abs() < 30.0)
        .unwrap();
    assert!(
        rule.rect.x >= line.pos.x - 1.0 && rule.rect.w < 50.0,
        "{rule:?}"
    );
    // A blue mark beside black words.
    let receipt = text(&o, "• +0 credits");
    assert_eq!(receipt.color, Color::hex(0x000000));
    assert_eq!(receipt.spans[0].color, Color::hex(0x0072b2));
    // A tag hugs its words.
    let tag = text(&o, "CONFIRMED");
    let ticket = behind(&o, tag);
    assert_eq!(ticket.fill, Color::hex(0x1f1a24));
    assert!(ticket.rect.w < 120.0, "{ticket:?}");
    // Icons: the coin keeps its paint; the arrow takes the gain look's blue; the book on
    // the Log button takes the button's black text.
    let images: Vec<_> = o.widgets.iter().flat_map(|l| &l.images).collect();
    assert_eq!(images.len(), 3, "{images:?}");
    assert_eq!(images[0].tint, None);
    assert_eq!(images[1].tint, Some(Color::hex(0x0072b2)));
    assert_eq!(images[2].tint, Some(Color::hex(0x000000)));
    let log = text(&o, "Log");
    assert!(
        images[2].rect.y + images[2].rect.h < log.pos.y,
        "the book sits above Log"
    );
}

/// Plays the bot's inputs, checking every note rebuilds from its recording, and returns
/// the session and the overlay after each tick.
fn play(reduced: bool) -> (Session, Vec<Overlay>) {
    let bot = InputsFile::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/ui-kit-inputs.toml"),
    )
    .unwrap();
    let mut s = Session::new(vec![kit()], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    v.reduced_motion = reduced;
    let mut overlays = vec![Overlay::default()];
    for tick in 1..=100 {
        v.scene(&s.world, 1.0, SIZE, 0.0);
        let mut inputs = vec![];
        for p in bot.pointers.iter().filter(|p| p.tick == tick) {
            inputs.extend(v.screens.bot_pointer(p).unwrap());
        }
        for c in bot.clicks.iter().filter(|c| c.tick == tick) {
            inputs.push(v.screens.bot_click(c).unwrap());
        }
        v.before_step(&s.world);
        s.step(&inputs).unwrap();
        let scene = v.scene(&s.world, 1.0, SIZE, 0.0);
        assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
        s.record_presentation(serde_json::to_value(&v.screens.presentation).unwrap());
        if let Some((_, words)) = bot.notes.iter().find(|n| n.0 == tick) {
            let moment = v.moment(1.0, 0.0, (1280, 720), 1.0).unwrap();
            s.note(words, Some(serde_json::to_value(&moment).unwrap()), None);
            let rec = s.recording().unwrap();
            let mut replay = Replay::new(&rec).unwrap();
            while replay.step().unwrap() {}
            let mut rebuilt = View::detached(replay.session.modules(), 1);
            assert_eq!(
                rebuilt.note_scene(&replay.session.world, &moment).overlay,
                scene.overlay,
                "note at tick {tick}"
            );
        }
        overlays.push(scene.overlay);
    }
    (s, overlays)
}

fn shelf(o: &Overlay) -> Option<&Panel> {
    let words = texts(o)
        .into_iter()
        .find(|t| t.text.starts_with("Earnings so far"))?;
    Some(behind(o, words))
}

#[test]
fn ui_motion_is_presentation_only_and_notes_rebuild_mid_motion() {
    let (moving, frames) = play(false);
    let (still, still_frames) = play(true);
    assert_eq!(moving.world.hash(), still.world.hash());
    let rec = moving.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    while replay.step().unwrap() {}
    assert_eq!(replay.session.world.hash(), moving.world.hash());

    // The shelf opens at tick 41: it fades and slides in, then settles.
    let settled = *shelf(&frames[55]).unwrap();
    assert_eq!(settled.opacity, 1.0);
    let coming = shelf(&frames[43]).unwrap();
    assert!(coming.opacity > 0.0 && coming.opacity < 1.0, "{coming:?}");
    assert!(coming.rect.y > settled.rect.y, "it rises into place");
    // It closes at tick 71 and fades out where it was, then is gone.
    let going = shelf(&frames[73]).unwrap();
    assert!(going.opacity > 0.0 && going.opacity < 1.0, "{going:?}");
    assert!(going.rect.y >= settled.rect.y);
    assert!(shelf(&frames[85]).is_none());
    // Credits count up after Earn (tick 16) and a flash lights `Earnings`.
    text(&frames[22], "Earnings: 1");
    let counting: i64 = texts(&frames[22])
        .into_iter()
        .find_map(|t| t.text.strip_prefix('+').and_then(|n| n.parse().ok()))
        .unwrap();
    assert!(counting > 120 && counting < 193, "{counting}");
    let flash = Color::hex(0xffe9a6);
    assert!(
        panels(&frames[22])
            .iter()
            .any(|p| p.fill == flash && p.opacity < 1.0)
    );
    assert_eq!(text(&frames[40], "+193").color, Color::hex(0xffffff));
    assert!(!panels(&frames[40]).iter().any(|p| p.fill == flash));

    // Reduced motion: the same world and boxes, but nothing moves, counts or flashes.
    let still_shelf = shelf(&still_frames[43]).unwrap();
    assert_eq!(still_shelf.opacity, 1.0);
    assert_eq!(still_shelf.rect, settled.rect);
    assert!(shelf(&still_frames[73]).is_none());
    text(&still_frames[22], "+193");
    assert!(!panels(&still_frames[22]).iter().any(|p| p.fill == flash));
}

#[test]
fn a_button_lifts_under_the_pointer_and_sinks_while_pressed() {
    let mut s = Session::new(vec![kit()], 1).unwrap();
    s.step(&[]).unwrap();
    let mut v = View::detached(s.modules(), 1);
    let rest = v.scene(&s.world, 1.0, SIZE, 0.0).overlay;
    let label = text(&rest, "Earn 73").clone();
    let at_rest = *behind(&rest, &label);
    assert_eq!(at_rest.fill, Color::hex(0xf2b43c));
    let point = Vec2::new(label.pos.x + 10.0, label.pos.y + 8.0);
    v.screens.pointer_move(point, 1);
    let over = v.scene(&s.world, 1.0, SIZE, 0.0).overlay;
    let lifted = text(&over, "Earn 73");
    assert_eq!(lifted.pos.y, label.pos.y - 2.0);
    let hovered = behind(&over, lifted);
    assert_eq!(hovered.fill, Color::hex(0xffd257));
    assert_eq!(hovered.rect.y, at_rest.rect.y - 2.0);
    let input = v.screens.pointer_down(point, 1).unwrap();
    assert_eq!(input.name, "kit.earn");
    let down = v.scene(&s.world, 1.0, SIZE, 0.0).overlay;
    assert_eq!(text(&down, "Earn 73").pos.y, label.pos.y + 2.0);
    v.screens.pointer_up(point, 1);
    let up = v.scene(&s.world, 1.0, SIZE, 0.0).overlay;
    assert_eq!(text(&up, "Earn 73").pos.y, label.pos.y - 2.0);
    // Hit testing never moves: the button's box is where layout put it.
    assert_eq!(v.screens.click(point, 1).unwrap().name, "kit.earn");
}

/// The kit with one file replaced, checked at its first screen.
fn problems(file: &str, find: &str, replace: &str) -> String {
    let mut m = kit();
    let text = m.files.0.get_mut(file).unwrap();
    assert!(text.contains(find), "{file} has no `{find}`");
    *text = text.replace(find, replace);
    let mut s = Session::new(vec![m], 1).unwrap();
    s.step(&[]).unwrap();
    let mut v = View::detached(s.modules(), 1);
    v.scene(&s.world, 1.0, SIZE, 0.0);
    let mut all = v.assets.problems.clone();
    all.extend(v.screens.problems.clone());
    all.join("\n")
}

#[test]
fn unreadable_looks_spans_marks_and_icons_are_findings() {
    for (file, find, replace, expected) in [
        // A look without a fill is checked on the surface it is drawn on.
        (
            "style.toml",
            "[screen.looks.gain]\ntext = \"#0072b2\"",
            "[screen.looks.gain]\ntext = \"#f7e8c8\"",
            "needs contrast at least 3:1",
        ),
        (
            "screen.luau",
            "{ text = \"Odo\", look = \"name\" }",
            "{ text = \"Odo\", look = \"gain\" }",
            "is a mark: symbols only",
        ),
        (
            "screen.luau",
            "look = \"ticket\"",
            "look = \"absent\"",
            "look `absent` is not in [screen.looks]",
        ),
        (
            "icons/up.svg",
            "<path",
            "<text>up</text><path",
            "<text> is not supported in icons",
        ),
        (
            "icons.toml",
            "icons/coin.svg",
            "icons/gone.svg",
            "`icons/gone.svg` is not in the module",
        ),
        (
            "screen.luau",
            "icon = \"up\", look = \"gain\", size = 20",
            "icon = \"up\", look = \"gain\", size = 12",
            "icon is smaller than 16 logical pixels",
        ),
        (
            "style.toml",
            "[screen.looks.chip]\nfill = \"#1f1a24\"\ntext = \"#ffffff\"",
            "[screen.looks.chip]\nfill = \"#1f1a24\"\ntext = \"#303030\"",
            "[screen.looks.chip] text on fill needs contrast at least 4.5:1",
        ),
    ] {
        let found = problems(file, find, replace);
        assert!(found.contains(expected), "{expected}:\n{found}");
    }
}

//! A note on an accepting drop target: at pickup it shows under `Drop here` on that
//! target only, layout has reserved it so no target moves, and a mid-drag note redraws
//! identically on replay.
use galaxy_module::module::Module;
use galaxy_render::{Overlay, Rect, glam::Vec2};
use galaxy_sim::{BuildStamp, Replay, Session, scenario::InputsFile};
use galaxy_view::View;
use std::path::Path;

const NOTE: &str = "Farther away: 2 more steps";

fn size() -> Vec2 {
    Vec2::new(1280.0, 720.0)
}

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play")
}

fn rect_of(v: &View, id: &str) -> Rect {
    fn find(b: &galaxy_view::screens::Boxed, id: &str) -> Option<Rect> {
        if b.widget.id.as_deref() == Some(id) {
            return Some(b.rect);
        }
        b.children.iter().find_map(|c| find(c, id))
    }
    v.screens
        .boxes
        .iter()
        .find_map(|b| find(b, id))
        .unwrap_or_else(|| panic!("no widget {id}"))
}

fn texts(overlay: &Overlay) -> Vec<String> {
    overlay
        .widgets
        .iter()
        .flat_map(|l| &l.texts)
        .map(|t| t.text.clone())
        .collect()
}

/// The bot file the window run plays: pickup, a mid-drag note, a drop on the noted slot,
/// a pickup from inside it and a drop on the other slot.
#[test]
fn a_note_shows_at_pickup_on_its_target_only_and_nothing_moves() {
    let module = Module::read(&root().join("drop-notes/demo")).unwrap();
    let mut s = Session::new(vec![module], 1).unwrap();
    s.start_recording(BuildStamp::current());
    let mut v = View::detached(s.modules(), 1);
    let bot = InputsFile::read(&root().join("drop-notes-inputs.toml")).unwrap();
    v.scene(&s.world, 1.0, size(), 0.0);
    let (near, far) = (rect_of(&v, "near"), rect_of(&v, "far"));
    let mut notes = Vec::new();
    for tick in 1..=90 {
        v.before_step(&s.world);
        v.scene(&s.world, 1.0, size(), 0.0);
        let mut inputs = Vec::new();
        for p in bot.pointers.iter().filter(|p| p.tick == tick) {
            inputs.extend(v.bot_pointer(&s.world, p).unwrap());
        }
        s.step(&inputs).unwrap();
        let scene = v.scene(&s.world, 1.0, size(), 0.0);
        assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
        // Picking up, carrying and placing never move either slot.
        assert_eq!(
            (rect_of(&v, "near"), rect_of(&v, "far")),
            (near, far),
            "tick {tick}"
        );
        s.record_presentation(serde_json::to_value(&v.screens.presented).unwrap());
        let shown = texts(&scene.overlay);
        let dragging = (10..24).contains(&tick) || (50..60).contains(&tick);
        if dragging {
            assert!(
                shown.contains(&"Near slot\nDrop here".to_string()),
                "{shown:?}"
            );
            assert!(
                shown.contains(&format!("Far slot\nDrop here\n{NOTE}")),
                "{shown:?}"
            );
        } else {
            assert!(shown.contains(&"Near slot".to_string()), "{shown:?}");
            assert!(shown.contains(&"Far slot".to_string()), "{shown:?}");
        }
        assert_eq!(
            shown.iter().filter(|t| t.contains(NOTE)).count(),
            usize::from(dragging),
            "tick {tick}: {shown:?}"
        );
        if let Some((_, text)) = bot.notes.iter().find(|n| n.0 == tick) {
            let moment = v.moment(1.0, 0.0, (1280, 720), 1.0).unwrap();
            s.note(text, Some(serde_json::to_value(&moment).unwrap()), None);
            notes.push((tick, moment, scene.overlay));
        }
    }
    let tray = &s.world.component("demo.state", "demo.tray").unwrap().fields;
    assert_eq!((&tray["at"], &tray["drops"]), (&"near".into(), &2.into()));

    // Every note, the mid-drag ones included, redraws identically from the recording,
    // and the world ends the same.
    let rec = s.recording().unwrap();
    let mut replay = Replay::new(&rec).unwrap();
    let mut rebuilt = View::detached(replay.session.modules(), 1);
    for (tick, moment, overlay) in &notes {
        while replay.session.world.tick < *tick {
            rebuilt.before_step(&replay.session.world);
            replay.step().unwrap();
        }
        assert_eq!(
            rebuilt.note_scene(&replay.session.world, moment).overlay,
            *overlay,
            "note tick {tick}"
        );
    }
    while replay.step().unwrap() {}
    assert_eq!(replay.session.world.hash(), s.world.hash());
}

fn screen(widget: &str) -> Vec<String> {
    let mut files = galaxy_module::module::ModuleFiles::default();
    files.0.insert(
        "manifest.toml".into(),
        "id = 'test'\nversion = '0.1.0'\nmin_engine_version = '0.7.0'\nscripts = ['screen.luau']\nevents = ['drop']\n".into(),
    );
    files.0.insert(
        "screen.luau".into(),
        format!("return {{ screen = function(_, ui) return {widget} end }}"),
    );
    let module = Module::from_files(Path::new("test"), files).unwrap();
    let s = Session::new(vec![module], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    v.scene(&s.world, 1.0, size(), 0.0);
    v.screens.problems.clone()
}

#[test]
fn notes_describe_accepted_values_of_drop_targets_in_readable_text() {
    let target = |extra: &str| {
        format!(
            "ui.drop_target {{ id = 'slot', text = 'Slot', on = 'drop', width = 300, {extra} }}"
        )
    };
    for (widget, problem) in [
        (
            target("accepts = { a = 'Too wide' }, notes = { a = 'Slower' }"),
            "notes.a describes an accepted drop",
        ),
        (
            target("accepts = { a = '' }, notes = { b = 'Slower' }"),
            "notes.b describes an accepted drop",
        ),
        (
            target("accepts = { a = '' }, notes = { a = ' ' }"),
            "notes.a needs text",
        ),
        (
            target("accepts = { a = '' }, notes = { a = 3 }"),
            "notes maps accepted source values to text",
        ),
        (
            "ui.button { text = 'Go', on = 'drop', notes = { a = 'x' } }".to_string(),
            "only drop targets have notes",
        ),
        // The usual readability checks read every note.
        (
            target("accepts = { a = '' }, notes = { a = 'Slower \u{10ffff}' }"),
            "drop verdict has missing glyphs",
        ),
        (
            target("height = 90, accepts = { a = '' }, notes = { a = 'Slower' }"),
            "drop verdict is clipped",
        ),
    ] {
        let problems = screen(&widget);
        assert!(
            problems.iter().any(|p| p.contains(problem)),
            "{widget}: {problems:?}"
        );
    }
    assert_eq!(
        screen(&target(
            "accepts = { a = '', b = 'No' }, notes = { a = 'Slower' }"
        )),
        Vec::<String>::new()
    );
    // The same short target fits its verdicts; only the note's extra line is clipped.
    assert_eq!(
        screen(&target("height = 90, accepts = { a = '' }")),
        Vec::<String>::new()
    );
}

/// A module whose `Update` button rewrites the far slot's note, as a cost change would.
fn updating(first: &str, next: &str) -> Module {
    let mut files = galaxy_module::module::ModuleFiles::default();
    files.0.insert(
        "manifest.toml".into(),
        "id = 'test'\nversion = '0.1.0'\nmin_engine_version = '0.7.0'\nscripts = ['screen.luau']\nevents = ['update', 'place']\n[components.tray]\nchanged = 'bool'\n".into(),
    );
    files.0.insert(
        "start.toml".into(),
        "[entities.state]\ntray = { changed = false }\n".into(),
    );
    files.0.insert(
        "screen.luau".into(),
        format!(
            "return {{
    on = {{ update = function(_, ctx) ctx.patch('state', 'tray', {{ changed = true }}) end }},
    screen = function(ctx, ui)
        local changed = ctx.get('state', 'tray').changed
        return ui.column {{ width = 267, children = {{
            ui.button {{ id = 'update', text = 'Update', on = 'update' }},
            ui.drag_source {{ id = 'token', text = 'Token', value = 'token' }},
            ui.drop_target {{ id = 'far', text = 'Far slot', on = 'place',
                accepts = {{ token = '' }},
                notes = {{ token = if changed then {next:?} else {first:?} }} }},
            ui.drop_target {{ id = 'near', text = 'Near slot', on = 'place',
                accepts = {{ token = '' }} }},
        }} }}
    end,
}}"
        ),
    );
    Module::from_files(Path::new("test"), files).unwrap()
}

/// Hold the token, press Update, and return both slots' boxes before and after with the
/// problems the redraw reports.
fn update_while_holding(first: &str, next: &str) -> ((Rect, Rect), (Rect, Rect), Vec<String>) {
    use galaxy_sim::scenario::{Click, ClickTarget, Pointer};
    let mut s = Session::new(vec![updating(first, next)], 1).unwrap();
    let mut v = View::detached(s.modules(), 1);
    v.scene(&s.world, 1.0, size(), 0.0);
    assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
    let pointer = |tick, action: &str, widget: &str| Pointer {
        tick,
        action: action.into(),
        widget: widget.into(),
        delta: 0.0,
    };
    v.bot_pointer(&s.world, &pointer(2, "pickup", "token"))
        .unwrap();
    v.bot_pointer(&s.world, &pointer(2, "move", "point:600,500"))
        .unwrap();
    v.scene(&s.world, 1.0, size(), 0.0);
    assert!(v.screens.presentation.drag.is_some());
    assert!(v.screens.problems.is_empty(), "{:?}", v.screens.problems);
    let before = (rect_of(&v, "far"), rect_of(&v, "near"));
    let update = v
        .screens
        .bot_click(&Click {
            tick: 3,
            target: ClickTarget::Widget("update".into()),
        })
        .unwrap();
    s.step(&[update]).unwrap();
    let scene = v.scene(&s.world, 1.0, size(), 0.0);
    assert!(v.screens.presentation.drag.is_some());
    assert!(
        texts(&scene.overlay).contains(&format!("Far slot\nDrop here\n{next}")),
        "the update reached the note"
    );
    let after = (rect_of(&v, "far"), rect_of(&v, "near"));
    (before, after, v.screens.problems.clone())
}

#[test]
fn a_note_that_changes_mid_drag_moves_no_target() {
    // The note grows by a wrapped line: the targets keep the space reserved at pickup,
    // and the note that no longer fits it is a problem.
    let (before, after, problems) = update_while_holding(
        "Installation cost: 9 credits; mass after fitting: 2",
        "Installation cost: 10 credits; mass after fitting: 2",
    );
    assert_eq!(before, after);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("/far[") && p.ends_with("drop verdict is clipped")),
        "{problems:?}"
    );
    // A note that still fits, longer or shorter, moves nothing and reads fine.
    for next in ["Installation cost: 8 credits", "Cost: 10 credits; mass 2"] {
        let (before, after, problems) =
            update_while_holding("Installation cost: 9 credits; mass after fitting: 2", next);
        assert_eq!(before, after, "{next}");
        assert!(problems.is_empty(), "{next}: {problems:?}");
    }
}

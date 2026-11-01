//! `places-lab --shots <folder>`: a scripted run, headless at 1280x720, through the same
//! pointer path as the window: from the station into each room, talk to its person, do
//! the room's thing, back, and round again. Saves a PNG per step and reports what a
//! player would feel: how long each move takes, the clicks from the station to hearing
//! each person, whether the speaker stayed on screen and uncovered while their line
//! showed (a rule: the run fails if not), and frame time. Fails on any rule break or
//! script error too.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use render::Renderer;
use render::glam::Vec2;

use crate::app::App;
use crate::combo::Combo;

struct Shooter {
    r: Renderer,
    app: App,
    out: PathBuf,
    n: usize,
    failures: Vec<String>,
    frame_ms: Vec<f64>,
    /// Each shot's draw on the GPU, read back.
    render_ms: Vec<f64>,
    /// Hovers the sweeps tried.
    hovers: usize,
}

const DT: f32 = 1.0 / 60.0;
/// Frames a hover rests for: long enough for a module's lift and a sign's growth.
const HOVER_FRAMES: usize = 14;

impl Shooter {
    fn frame(&mut self) -> render::Scene {
        let t = Instant::now();
        let scene = self.app.frame(&mut self.r, DT);
        self.frame_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        scene
    }

    fn settle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.frame();
        }
    }

    /// Draws this frame to a numbered PNG and returns its path.
    fn shoot(&mut self, name: &str) -> PathBuf {
        let scene = self.frame();
        self.n += 1;
        let path = self.out.join(format!("{:02}-{name}.png", self.n));
        let t = Instant::now();
        let img = self.r.render_to_rgba(&scene);
        self.render_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        if let Err(e) = img.and_then(|img| img.save_png(&path)) {
            self.failures.push(format!("{name}: {e}"));
        }
        path
    }

    /// A shot of this state, its rule breaks, and a sweep hovering everything on it.
    fn save(&mut self, name: &str) {
        // Someone the player can see is on the move (a life variant's walk): let them
        // get where they are going, so the shot and its sweep see a settled room.
        for _ in 0..1200 {
            if !self.app.combo.life().settling(&self.app.model) {
                break;
            }
            self.frame();
        }
        let path = self.shoot(name);
        println!("shot {}", path.display());
        let problems = self.app.problems();
        for p in &problems.0 {
            self.failures
                .push(format!("{name}: rule: {}: {}", p.path, p.what));
        }
        if let Some(e) = &self.app.host.error {
            self.failures.push(format!("{name}: script: {e}"));
        }
        self.sweep(name);
    }

    fn problem_set(&self) -> Vec<String> {
        self.app
            .problems()
            .0
            .iter()
            .map(|p| format!("{}: {}", p.path, p.what))
            .collect()
    }

    /// Hovers everything the pointer can act on in this state, one at a time, so a
    /// hover nobody thought to script is still looked at. Each must be reachable, keep
    /// its hover while the pointer rests (no flicker), break no rule and run no Luau.
    /// A hover that breaks a rule is saved as a picture of its own.
    fn sweep(&mut self, state: &str) {
        if self.app.ix.drag.is_some() {
            return;
        }
        let pointer = self.app.ix.pointer;
        self.app.pointer_left();
        self.settle(HOVER_FRAMES);
        let base = self.problem_set();
        let runs = self.app.host.stats.view_runs;
        let (targets, unreachable) = self.app.targets();
        for u in unreachable {
            self.failures.push(format!(
                "{state}: {u} can't be reached: something covers all of it"
            ));
        }
        for t in targets {
            let name = t.id.clone().unwrap_or_else(|| t.path.clone());
            self.app.pointer_move(t.at);
            let mut lost = 0;
            for _ in 0..HOVER_FRAMES {
                self.frame();
                if self.app.ix.hover.as_deref() != Some(t.path.as_str()) {
                    lost += 1;
                }
            }
            self.hovers += 1;
            if lost > 0 {
                self.failures.push(format!(
                    "{state}: hovering {name} doesn't hold: the pointer was over something else on {lost} of {HOVER_FRAMES} frames"
                ));
            }
            let fresh: Vec<String> = self
                .problem_set()
                .into_iter()
                .filter(|p| !base.contains(p))
                .collect();
            if !fresh.is_empty() {
                let file: String = name
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                    .collect();
                let path = self.shoot(&format!("{state}--hover-{file}"));
                for p in fresh {
                    self.failures.push(format!(
                        "{state}: hovering {name}: rule: {p} (see {})",
                        path.display()
                    ));
                }
            }
        }
        let ran = self.app.host.stats.view_runs - runs;
        if ran != 0 {
            self.failures
                .push(format!("{state}: hovering ran the screen {ran} times"));
        }
        match pointer {
            Some(p) => self.app.pointer_move(p),
            None => self.app.pointer_left(),
        }
        self.settle(HOVER_FRAMES);
    }

    /// A widget's place: an id, a path from `describe` (`screen/panel[1]/button[2]`),
    /// or `x,y` in logical pixels.
    fn at(&mut self, id: &str) -> Vec2 {
        if let Some((x, y)) = id.split_once(',')
            && let (Ok(x), Ok(y)) = (x.trim().parse(), y.trim().parse())
        {
            return Vec2::new(x, y);
        }
        let by_path = || {
            let (targets, _) = self.app.targets();
            targets.into_iter().find(|t| t.path == id).map(|t| t.at)
        };
        match self.app.locate(id).or_else(by_path) {
            Some(p) => p,
            None => {
                self.failures.push(format!("no widget {id:?} on screen"));
                Vec2::ZERO
            }
        }
    }

    fn hover(&mut self, id: &str) {
        let p = self.at(id);
        self.app.pointer_move(p);
        self.settle(12);
    }

    fn click(&mut self, id: &str) {
        let p = self.at(id);
        self.app.pointer_move(p);
        self.frame();
        self.app.pointer_down();
        self.app.pointer_up();
        self.settle(2);
    }

    /// Picks up `from`, carries it to `to` over several frames and either saves a shot
    /// while it is held or lets go.
    fn drag(&mut self, from: &str, to: &str, shot: Option<&str>, let_go: bool) {
        let a = self.at(from);
        self.app.pointer_move(a);
        self.frame();
        self.app.pointer_down();
        self.app.pointer_move(a + Vec2::new(8.0, 4.0));
        self.settle(2);
        let runs_after_pickup = self.app.host.stats.view_runs;
        let b = self.at(to);
        for i in 1..=20 {
            let t = i as f32 / 20.0;
            self.app.pointer_move(a.lerp(b, t));
            self.frame();
        }
        let moved = self.app.host.stats.view_runs - runs_after_pickup;
        if moved != 0 {
            self.failures.push(format!(
                "dragging {from} ran the screen {moved} times while moving"
            ));
        }
        if let Some(name) = shot {
            self.save(name);
        }
        if let_go {
            self.app.pointer_up();
        } else {
            // Esc with the button still held: moving on and letting go over the target
            // must not pick the part up again or fit it.
            self.app.back();
            let events = self.app.events.len();
            self.app.pointer_move(b + Vec2::new(30.0, 30.0));
            self.frame();
            self.app.pointer_move(b);
            self.frame();
            self.app.pointer_up();
            self.frame();
            let after: Vec<String> = self.app.events[events..].to_vec();
            if self.app.ix.drag.is_some() || !after.is_empty() {
                self.failures.push(format!(
                    "after Esc, moving and letting go with the button held still acted on {from}: {after:?}"
                ));
            }
        }
        let held = self.app.measures.runs_last_drag;
        println!("drag {from} -> {to}: screen runs while held {held} (moving: {moved})");
        if held != 1 {
            self.failures.push(format!(
                "dragging {from}: the screen ran {held} times while held (expected 1, at pick-up)"
            ));
        }
        self.settle(30);
    }
}

impl Shooter {
    /// Frames until the move's view has landed (and a few more for the screen).
    fn arrive(&mut self) {
        for _ in 0..600 {
            if !self.app.busy() {
                break;
            }
            self.frame();
        }
        self.settle(4);
    }

    /// A click that moves somewhere: waits for the move to land.
    fn go(&mut self, id: &str) {
        self.click(id);
        self.arrive();
    }

    /// The talk on screen now, if any: who speaks.
    fn speaker(&self) -> Option<String> {
        self.app.line.as_ref().map(|l| l.who.clone())
    }

    /// From the station, as few clicks as a player needs to hear `who`: their room, and
    /// them if they don't speak first. Returns the clicks and the seconds it took.
    fn reach(&mut self, room_key: &str, who: &str) -> (u64, f32) {
        let (c0, t0) = (self.app.measures.clicks, self.app.time);
        self.go(room_key);
        if self.speaker().as_deref() != Some(who) {
            self.click(&format!("person_{who}"));
        }
        if self.speaker().as_deref() != Some(who) {
            self.failures
                .push(format!("{who} doesn't speak after {room_key} and a click on them"));
        }
        (self.app.measures.clicks - c0, self.app.time - t0)
    }

    /// Clicks through the talk on screen until it ends.
    fn listen(&mut self) {
        for _ in 0..12 {
            if self.app.line.is_none() {
                return;
            }
            self.click("bubble");
        }
        self.failures.push("a talk didn't end after 12 clicks".into());
    }
}

fn shooter(dir: PathBuf, out: PathBuf, combo: Combo) -> Result<Shooter, String> {
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut r = Renderer::new_headless(1280, 720).map_err(|e| e.to_string())?;
    let app = App::new(&mut r, dir, combo)?;
    let mut s = Shooter {
        r,
        app,
        out,
        n: 0,
        failures: Vec::new(),
        frame_ms: Vec::new(),
        render_ms: Vec::new(),
        hovers: 0,
    };
    s.app.show_stats = false;
    s.settle(30);
    Ok(s)
}

/// `places-lab --look <folder> "<steps>"`: plays the steps headless and, at each `shot`,
/// saves a PNG and prints what is on screen as JSON, so an agent can see what a hover,
/// click or drag does. Steps, split by `;`:
///
/// - `hover W`, `click W`, `press W` (hold the button down), `release`, `leave`
/// - `drag W V` (pick W up and hold it over V), `drop` (let go)
/// - `back`, `wait N` (frames), `arrive` (wait for a move to land), `zoom STEPS [W]`
/// - `go PLACE` (move there as a script would), `key N` (cycle choice N, 1-4),
///   `combo SPEC` (pick variants: `cut,staged,bubble,still`)
/// - `shot [NAME]` (a PNG and the JSON), `sweep` (hover everything here, report breaks)
///
/// `W` is a widget id, a path from the JSON, or `x,y`. Without a final `shot`, one is
/// taken at the end.
pub fn look(dir: PathBuf, out: PathBuf, script: &str, combo: Combo) -> ExitCode {
    let mut s = match shooter(dir, out, combo) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("places-lab: {e}");
            return ExitCode::FAILURE;
        }
    };
    let viewport = s.r.logical_size();
    let steps: Vec<&str> = script
        .split(';')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let mut shot_last = false;
    for step in steps {
        let words: Vec<&str> = step.split_whitespace().collect();
        shot_last = false;
        match words.as_slice() {
            ["hover", w] => {
                let p = s.at(w);
                s.app.pointer_move(p);
                s.settle(HOVER_FRAMES);
            }
            ["click", w] => s.click(w),
            ["press", w] => {
                let p = s.at(w);
                s.app.pointer_move(p);
                s.frame();
                s.app.pointer_down();
                s.settle(2);
            }
            ["release"] | ["drop"] => {
                s.app.pointer_up();
                s.settle(2);
            }
            ["leave"] => {
                s.app.pointer_left();
                s.settle(2);
            }
            ["drag", w, v] => {
                let a = s.at(w);
                s.app.pointer_move(a);
                s.frame();
                s.app.pointer_down();
                s.app.pointer_move(a + Vec2::new(8.0, 4.0));
                s.settle(2);
                let b = s.at(v);
                for i in 1..=20 {
                    s.app.pointer_move(a.lerp(b, i as f32 / 20.0));
                    s.frame();
                }
            }
            ["back"] => {
                s.app.back();
                s.settle(4);
            }
            ["wait", n] => s.settle(n.parse().unwrap_or(30)),
            ["arrive"] => s.arrive(),
            ["go", place] => {
                s.app.go(place);
                s.settle(2);
            }
            ["key", n] => match n.parse::<usize>() {
                Ok(n @ 1..=4) => {
                    s.app.cycle(n - 1, false);
                    s.settle(2);
                }
                _ => s.failures.push(format!("key {n}: choices are 1 to 4")),
            },
            ["combo", spec] => {
                if let Err(e) = s.app.combo.set(spec) {
                    s.failures.push(e);
                }
                s.app.cycle(0, false);
                s.app.cycle(0, true);
                s.settle(2);
            }
            ["zoom", steps, rest @ ..] => {
                let at = rest.first().map(|w| s.at(w));
                s.app
                    .world
                    .zoom_at(steps.parse().unwrap_or(0.0), at, viewport);
                s.settle(30);
            }
            ["shot", rest @ ..] if rest.len() <= 1 => {
                let path = s.shoot(rest.first().copied().unwrap_or("shot"));
                print_look(&s, &path);
                shot_last = true;
            }
            ["sweep"] => {
                let before = s.hovers;
                s.sweep("sweep");
                println!(
                    "{}",
                    serde_json::json!({ "swept": s.hovers - before, "failures": s.failures })
                );
            }
            _ => s.failures.push(format!("unknown step {step:?}")),
        }
    }
    if !shot_last {
        let path = s.shoot("last");
        print_look(&s, &path);
    }
    if s.failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        for f in &s.failures {
            eprintln!("FAIL {f}");
        }
        ExitCode::FAILURE
    }
}

fn print_look(s: &Shooter, path: &std::path::Path) {
    let mut v = s.app.describe();
    v["shot"] = serde_json::json!(path.display().to_string());
    println!(
        "{}",
        serde_json::to_string_pretty(&v).unwrap_or_else(|e| e.to_string())
    );
}

/// The station's rooms as the run visits them: the station key's id, the room, the
/// person to hear, and a short name for the shots.
const VISITS: [(&str, &str, &str); 4] = [
    ("key_office_block", "office", "ilse"),
    ("key_market_block", "market", "juno"),
    ("key_wharf_arm", "cradle", "orrin"),
    ("key_canteen_block", "canteen", "bel"),
];

pub fn run(dir: PathBuf, out: PathBuf, combo: Combo) -> ExitCode {
    let mut s = match shooter(dir, out, combo) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("places-lab: {e}");
            return ExitCode::FAILURE;
        }
    };
    let spec = s.app.combo.spec();
    println!("combination {spec}");
    s.save("station");
    s.hover("spot_office_block");
    s.save("station-hover-office");

    // First visits: each room, its person, the room's thing, and back.
    let mut first: Vec<(String, u64, f32)> = Vec::new();
    for (key, room, who) in VISITS {
        let (c0, t0) = (s.app.measures.clicks, s.app.time);
        s.app.pointer_left();
        s.click(key);
        if room == "office" {
            // Part way: what the move looks like.
            s.settle(20);
            s.shoot("on-the-way-to-office");
        }
        s.arrive();
        if s.speaker().as_deref() != Some(who) {
            s.click(&format!("person_{who}"));
        }
        first.push((who.into(), s.app.measures.clicks - c0, s.app.time - t0));
        s.app.pointer_left();
        s.save(&format!("{room}-talk"));
        s.listen();
        s.app.pointer_left();
        match room {
            "office" => s.save("office-after-welcome"),
            "market" => {
                s.click("sellall_ice");
                s.click("buy1_rations");
                s.app.pointer_left();
                s.save("market-traded");
                // Pim came here after Ilse's welcome.
                let (c, t) = (s.app.measures.clicks, s.app.time);
                s.click("person_pim");
                first.push(("pim".into(), s.app.measures.clicks - c, s.app.time - t));
                s.app.pointer_left();
                s.save("market-pim-talks");
                s.listen();
            }
            "cradle" => {
                s.drag("rack_lens", "socket_nose", Some("cradle-drag-lens"), true);
                s.app.pointer_left();
                s.save("cradle-lens-fitted");
            }
            "canteen" => {
                s.click("buy_bowl");
                s.app.pointer_left();
                s.save("canteen-bowl");
            }
            _ => {}
        }
        s.go("back");
        if s.app.model.here().kind != crate::places::Kind::Station {
            s.failures
                .push(format!("Station from the {room} didn't lead back to the station"));
        }
    }
    s.app.pointer_left();
    s.save("station-after-rooms");

    // Again: the people don't greet you twice; talking takes a click on them.
    let mut again: Vec<(String, u64, f32)> = Vec::new();
    for (key, room, who) in VISITS {
        let (c, t) = s.reach(key, who);
        again.push((who.into(), c, t));
        if room == "cradle" {
            s.app.pointer_left();
            s.save("cradle-talk-again");
        }
        s.listen();
        s.go("back");
    }

    // What the lab measured.
    let st = &s.app.host.stats;
    let m = s.app.measures.clone();
    let mut f = s.frame_ms.clone();
    f.sort_by(|a, b| a.total_cmp(b));
    let p = |q: f64| {
        f.get(((f.len() as f64 - 1.0) * q) as usize)
            .copied()
            .unwrap_or(0.0)
    };
    println!("\nwhat a player feels, with {spec}:");
    println!("  moves (the view's way from one place to the next):");
    for (a, b, t) in &m.moves {
        println!("    {a} -> {b}: {t:.2} s");
    }
    println!("  from the station to hearing each person, first visit:");
    for (who, c, t) in &first {
        let note = if who == "pim" {
            " (already in the market, after Juno)"
        } else {
            ""
        };
        println!("    {who}: {c} clicks, {t:.2} s{note}");
    }
    println!("  and coming back to talk again:");
    for (who, c, t) in &again {
        println!("    {who}: {c} clicks, {t:.2} s");
    }
    println!(
        "  the speaker on screen and uncovered: {} of {} frames with a line{}",
        m.line_frames - m.speaker_hidden,
        m.line_frames,
        if m.speaker_hidden == 0 { "" } else { " (FAIL)" }
    );
    println!(
        "  frame build (scene and UI, CPU, no GPU): median {:.2} ms, p95 {:.2} ms, max {:.2} ms over {} frames",
        p(0.5),
        p(0.95),
        p(1.0),
        s.frame_ms.len()
    );
    let mut g = s.render_ms.clone();
    g.sort_by(|a, b| a.total_cmp(b));
    println!(
        "  a shot drawn on the GPU and read back: median {:.1} ms over {} shots ({})",
        g.get(g.len() / 2).copied().unwrap_or(0.0),
        g.len(),
        s.r.adapter_info()
    );
    println!(
        "screen runs {} (max {:.2} ms) for {} events; layout max {:.2} ms; hovers swept {}",
        st.view_runs, st.max_view_ms, st.event_runs, m.max_layout_ms, s.hovers
    );
    if m.speaker_hidden > 0 {
        s.failures.push(format!(
            "the speaker was off screen or covered on {} frames",
            m.speaker_hidden
        ));
    }
    println!("events: {}", s.app.events.join(" | "));
    if s.failures.is_empty() {
        println!("PASS");
        ExitCode::SUCCESS
    } else {
        for f in &s.failures {
            println!("FAIL {f}");
        }
        ExitCode::FAILURE
    }
}

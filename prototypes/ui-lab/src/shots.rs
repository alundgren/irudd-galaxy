//! `ui-lab --shots <folder>`: a scripted run through every screen, headless at 1280x720,
//! through the same pointer path as the window. Saves a PNG per step, prints what the lab
//! measured, and fails on any rule break, script error or drag that ran the screen more
//! than once.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use render::Renderer;
use render::glam::Vec2;

use crate::app::App;

struct Shooter {
    r: Renderer,
    app: App,
    out: PathBuf,
    n: usize,
    failures: Vec<String>,
    frame_ms: Vec<f64>,
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
        if let Err(e) = self
            .r
            .render_to_rgba(&scene)
            .and_then(|img| img.save_png(&path))
        {
            self.failures.push(format!("{name}: {e}"));
        }
        path
    }

    /// A shot of this state, its rule breaks, and a sweep hovering everything on it.
    fn save(&mut self, name: &str) {
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

fn shooter(dir: PathBuf, out: PathBuf) -> Result<Shooter, String> {
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut r = Renderer::new_headless(1280, 720).map_err(|e| e.to_string())?;
    let app = App::new(&mut r, dir);
    let mut s = Shooter {
        r,
        app,
        out,
        n: 0,
        failures: Vec::new(),
        frame_ms: Vec::new(),
        hovers: 0,
    };
    s.app.show_stats = false;
    s.settle(30);
    Ok(s)
}

/// `ui-lab --look <folder> "<steps>"`: plays the steps headless and, at each `shot`,
/// saves a PNG and prints what is on screen as JSON, so an agent can see what a hover,
/// click or drag does. Steps, split by `;`:
///
/// - `hover W`, `click W`, `press W` (hold the button down), `release`, `leave`
/// - `drag W V` (pick W up and hold it over V), `drop` (let go)
/// - `back`, `wait N` (frames), `zoom STEPS [W]` (the wheel, toward W)
/// - `shot [NAME]` (a PNG and the JSON), `sweep` (hover everything here, report breaks)
///
/// `W` is a widget id, a path from the JSON, or `x,y`. Without a final `shot`, one is
/// taken at the end.
pub fn look(dir: PathBuf, out: PathBuf, script: &str) -> ExitCode {
    let mut s = match shooter(dir, out) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ui-lab: {e}");
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
                s.settle(30);
            }
            ["wait", n] => s.settle(n.parse().unwrap_or(30)),
            ["zoom", steps, rest @ ..] => {
                let at = rest.first().map(|w| s.at(w));
                s.app
                    .world
                    .zoom_at(steps.parse().unwrap_or(0.0), at, viewport);
                s.settle(60);
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

pub fn run(dir: PathBuf, out: PathBuf) -> ExitCode {
    let mut s = match shooter(dir, out) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ui-lab: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Docked: the welcome, one line at a time; the goal appears after it.
    s.settle(30);
    s.save("docked-welcome");
    // A hovered sign grows clear of the bubble, or in front of it, never mixed with it.
    for spot in ["spot_canteen", "spot_office", "spot_wharf"] {
        s.hover(spot);
        s.save(&format!("hover-{}-during-talk", &spot[5..]));
    }
    s.click("bubble");
    s.save("talk-second-line");
    for _ in 0..3 {
        s.click("bubble");
    }
    s.app.pointer_left();
    s.save("goal-after-talk");
    // The wheel's range at the station: the signs follow the scene with no screen runs.
    let runs = s.app.host.stats.view_runs;
    let viewport = s.r.logical_size();
    for (steps, at, name) in [
        (8.0, "spot_market", "zoomed-in-at-market"),
        (-6.0, "spot_office", "zoomed-in-less"),
        (-12.0, "spot_office", "zoomed-out"),
    ] {
        let p = s.at(at);
        s.app.world.zoom_at(steps, Some(p), viewport);
        s.settle(60);
        s.save(name);
    }
    s.hover("spot_market");
    s.save("zoomed-out-hover-market");
    s.app.pointer_left();
    s.app.world.set_wheel(1.0);
    s.settle(60);
    let zoom_runs = s.app.host.stats.view_runs - runs;
    if zoom_runs != 0 {
        s.failures
            .push(format!("zooming ran the screen {zoom_runs} times"));
    }
    s.hover("spot_market");
    s.save("hover-market-module");
    s.hover("key_wharf");
    s.save("key-ribbon-headline");

    // The market.
    s.click("key_market");
    s.settle(40);
    s.app.pointer_left();
    s.save("market");
    s.click("bubble");
    s.click("sellall_ice");
    s.app.pointer_left();
    s.save("market-sold-ice");
    s.click("buymax_plate");
    s.hover("buy1_rations");
    s.save("market-hold-full-resting-buy");

    // The cradle.
    s.app.back();
    s.settle(30);
    s.click("key_wharf");
    s.settle(40);
    s.app.pointer_left();
    s.save("cradle");
    s.click("bubble");
    s.drag(
        "locker_seam_lamp",
        "socket_utility",
        Some("drag-refused-power"),
        true,
    );
    s.drag("rack_lens", "socket_nose", None, false);
    s.drag("rack_lens", "socket_nose", Some("drag-lens-preview"), true);
    s.app.pointer_left();
    s.save("lens-fitted");
    s.drag("rack_pods", "socket_belly", None, true);
    s.app.pointer_left();
    s.save("pods-fitted");

    // Fill the bigger hold, then try to take the pods off.
    s.app.back();
    s.settle(30);
    s.click("key_market");
    s.settle(40);
    s.click("buymax_rations");
    s.app.back();
    s.settle(30);
    s.click("key_wharf");
    s.settle(40);
    s.drag(
        "fitted_belly",
        "locker",
        Some("drag-pods-off-refused-cargo"),
        true,
    );
    s.app.pointer_left();
    s.save("cradle-after-refusal");

    // The smaller rooms, and the log.
    s.app.back();
    s.settle(30);
    s.click("key_notices");
    s.settle(40);
    s.app.pointer_left();
    s.save("notices");
    s.app.world.zoom_at(-12.0, None, viewport);
    s.settle(60);
    s.save("notices-zoomed-out");
    s.app.world.set_wheel(1.0);
    s.settle(60);
    s.app.back();
    s.settle(30);
    s.click("key_canteen");
    s.settle(40);
    s.app.pointer_left();
    s.save("canteen-closed");
    s.click("bubble");
    s.app.back();
    s.settle(30);
    s.click("log");
    s.app.pointer_left();
    s.save("log-open");

    // What the lab measured.
    let st = &s.app.host.stats;
    let m = &s.app.measures;
    let mut f = s.frame_ms.clone();
    f.sort_by(|a, b| a.total_cmp(b));
    let p = |q: f64| {
        f.get(((f.len() as f64 - 1.0) * q) as usize)
            .copied()
            .unwrap_or(0.0)
    };
    println!(
        "screen runs {} (max {:.2} ms) for {} events and {} drags over {} frames; layout max {:.2} ms",
        st.view_runs,
        st.max_view_ms,
        st.event_runs,
        m.drags,
        s.frame_ms.len(),
        m.max_layout_ms
    );
    println!(
        "frame build (scene and UI, no GPU) median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        p(0.5),
        p(0.95),
        p(1.0)
    );
    println!("hovers swept: {}", s.hovers);
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

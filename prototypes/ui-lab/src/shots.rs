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
}

const DT: f32 = 1.0 / 60.0;

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

    fn save(&mut self, name: &str) {
        let scene = self.frame();
        self.n += 1;
        let path = self.out.join(format!("{:02}-{name}.png", self.n));
        match self
            .r
            .render_to_rgba(&scene)
            .and_then(|img| img.save_png(&path))
        {
            Ok(()) => println!("shot {}", path.display()),
            Err(e) => self.failures.push(format!("{name}: {e}")),
        }
        let problems = self.app.problems();
        for p in &problems.0 {
            self.failures
                .push(format!("{name}: rule: {}: {}", p.path, p.what));
        }
        if let Some(e) = &self.app.host.error {
            self.failures.push(format!("{name}: script: {e}"));
        }
    }

    fn at(&mut self, id: &str) -> Vec2 {
        match self.app.locate(id) {
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
            self.app.back();
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

pub fn run(dir: PathBuf, out: PathBuf) -> ExitCode {
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("ui-lab: {}: {e}", out.display());
        return ExitCode::FAILURE;
    }
    let mut r = match Renderer::new_headless(1280, 720) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ui-lab: {e}");
            return ExitCode::FAILURE;
        }
    };
    let app = App::new(&mut r, dir);
    let mut s = Shooter {
        r,
        app,
        out,
        n: 0,
        failures: Vec::new(),
        frame_ms: Vec::new(),
    };
    s.app.show_stats = false;

    // Docked: the welcome, one line at a time; the goal appears after it.
    s.settle(30);
    s.save("docked-welcome");
    s.click("bubble");
    s.save("talk-second-line");
    for _ in 0..3 {
        s.click("bubble");
    }
    s.app.pointer_left();
    s.save("goal-after-talk");
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

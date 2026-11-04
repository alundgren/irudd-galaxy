//! UI motion: panels and bubbles coming and going, buttons under the pointer, value cues.
//!
//! All of it is presentation. Its state lives in the presentation snapshot beside scroll
//! offsets and drags, timed by the screen clock (tick plus interpolation), so a note,
//! audit and film redraw a mid-fade frame exactly and the world never sees it.
use galaxy_script::screens::Widget;
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    /// Reduced motion: nothing slides, fades, lifts, counts or flashes.
    #[serde(default)]
    pub reduced: bool,
    /// Whether the first frame has been seen: what it shows is already there.
    #[serde(default)]
    pub primed: bool,
    /// Each shown panel or bubble, by key, with the clock time it came in.
    #[serde(default)]
    pub shown: BTreeMap<String, f32>,
    /// Panels and bubbles fading out after their screen dropped them.
    #[serde(default)]
    pub leaving: Vec<Leaving>,
    /// Each cued widget's last value and its running cue.
    #[serde(default)]
    pub values: BTreeMap<String, Cue>,
    /// The button held down, by path.
    #[serde(default)]
    pub pressed: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Leaving {
    pub key: String,
    /// The widget as it was last drawn, its icons resolved.
    pub tree: Widget,
    /// Its box when it left.
    pub rect: [f32; 4],
    pub start: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub value: String,
    pub from: f64,
    pub to: f64,
    pub start: f32,
}

/// Long before any clock time: things already shown when the screens first drew.
const SETTLED: f32 = -1000.0;
/// At most this many panels fade out at once; older ones just go.
const MAX_LEAVING: usize = 8;

/// A panel's or bubble's identity across rebuilds: its script and id, else its path.
pub(super) fn key(w: &Widget) -> String {
    match &w.id {
        Some(id) => format!("{}#{id}", w.path.split("/screen").next().unwrap_or("")),
        None => w.path.clone(),
    }
}

fn animated(kind: &str) -> bool {
    matches!(kind, "panel" | "scroll" | "bubble")
}

/// The outermost panels and bubbles: they move, and everything in them moves with them.
pub(super) fn surfaces<'a>(b: &'a Boxed, out: &mut Vec<&'a Boxed>) {
    if b.hidden || b.widget.yields {
        return;
    }
    if animated(&b.widget.kind) {
        out.push(b);
        return;
    }
    for c in &b.children {
        surfaces(c, out);
    }
}

fn cued<'a>(b: &'a Boxed, out: &mut Vec<&'a Boxed>) {
    if b.hidden {
        return;
    }
    if b.widget.cue.is_some() {
        out.push(b);
    }
    for c in &b.children {
        cued(c, out);
    }
}

pub(super) fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How far a delta's count has run, as the text it shows.
pub(super) fn counted(cue: &Cue, t: f32) -> String {
    let v = cue.from + (cue.to - cue.from) * smooth(t) as f64;
    let decimals = format!("{}", cue.to.abs())
        .split_once('.')
        .map_or(0, |(_, d)| d.len());
    if decimals == 0 && cue.from.fract() == 0.0 {
        let v = v.round();
        format!("{:+}", if v == 0.0 { 0.0 } else { v })
    } else {
        format!("{v:+.decimals$}")
    }
}

/// A widget's shown value for cues: a delta's number, else its words.
fn value(b: &Boxed) -> (String, f64) {
    if b.widget.kind == "delta" {
        (format!("{:+}", b.widget.delta), b.widget.delta)
    } else {
        (b.widget.text.clone(), 0.0)
    }
}

impl Screens {
    /// After this frame's boxes are placed: note what came, went and changed.
    pub(super) fn update_motion(&mut self, previous: &[Boxed]) {
        let clock = self.clock;
        let style = &self.style;
        let motion = &mut self.presentation.motion;
        if self.reduced_motion {
            motion.reduced = true;
        }
        let on = !motion.reduced;
        let mut now = Vec::new();
        for b in &self.boxes {
            surfaces(b, &mut now);
        }
        let now: BTreeMap<String, &Boxed> = now.into_iter().map(|b| (key(&b.widget), b)).collect();
        let primed = motion.primed;
        motion.primed = true;
        for k in now.keys() {
            motion
                .shown
                .entry(k.clone())
                .or_insert(if primed { clock } else { SETTLED });
        }
        let gone: Vec<String> = motion
            .shown
            .keys()
            .filter(|k| !now.contains_key(*k))
            .cloned()
            .collect();
        if !gone.is_empty() {
            let mut before = Vec::new();
            for b in previous {
                surfaces(b, &mut before);
            }
            for k in gone {
                motion.shown.remove(&k);
                if !(on && style.motion_out > 0.0) {
                    continue;
                }
                if let Some(b) = before.iter().find(|b| key(&b.widget) == k) {
                    motion.leaving.push(Leaving {
                        key: k,
                        tree: b.widget.clone(),
                        rect: [b.rect.x, b.rect.y, b.rect.w, b.rect.h],
                        start: clock,
                    });
                }
            }
        }
        let out = style.motion_out;
        motion
            .leaving
            .retain(|l| on && clock < l.start + out && !now.contains_key(&l.key));
        let extra = motion.leaving.len().saturating_sub(MAX_LEAVING);
        motion.leaving.drain(..extra);
        // Value cues: a change starts one; the first sight of a value doesn't.
        let mut cues = Vec::new();
        for b in &self.boxes {
            cued(b, &mut cues);
        }
        let mut seen = BTreeMap::new();
        for b in cues {
            let k = key(&b.widget);
            let (text, number) = value(b);
            let cue = match motion.values.get(&k) {
                Some(c) if c.value == text => c.clone(),
                Some(c) => Cue {
                    value: text,
                    from: c.to,
                    to: number,
                    start: clock,
                },
                None => Cue {
                    value: text,
                    from: number,
                    to: number,
                    start: SETTLED,
                },
            };
            seen.insert(k, cue);
        }
        motion.values = seen;
    }

    /// How far into its arrival a panel is (1 settled), and how far into a cue.
    pub(super) fn arrival(&self, b: &Boxed) -> f32 {
        let m = &self.presentation.motion;
        if m.reduced || self.style.motion_in <= 0.0 {
            return 1.0;
        }
        m.shown.get(&key(&b.widget)).map_or(1.0, |start| {
            smooth((self.clock - start) / self.style.motion_in)
        })
    }

    pub(super) fn cue(&self, b: &Boxed) -> Option<(&Cue, f32)> {
        let m = &self.presentation.motion;
        if m.reduced || self.style.cue_time <= 0.0 {
            return None;
        }
        let cue = m.values.get(&key(&b.widget))?;
        let t = (self.clock - cue.start) / self.style.cue_time;
        (0.0..1.0).contains(&t).then_some((cue, t))
    }

    /// The leaving panels laid out again where they were, each with how far out it is.
    pub(super) fn leaving(&mut self, size: Vec2) -> Vec<(Boxed, f32)> {
        let leaving = self.presentation.motion.leaving.clone();
        let mut out = Vec::new();
        for l in leaving {
            let mut tree = l.tree.clone();
            tree.anchor = None;
            tree.align = None;
            tree.x = None;
            tree.y = None;
            tree.yields = false;
            tree.width = Some(l.rect[2]);
            let style = Arc::new(self.style.clone());
            let mut b = layout(
                &tree,
                l.rect[2],
                &mut Kit {
                    measure: &mut self.measure,
                    style: &style,
                    kept: &BTreeMap::new(),
                    icons: &self.icons,
                },
            );
            let mut quiet = Vec::new();
            place(
                &mut b,
                Vec2::new(l.rect[0], l.rect[1]),
                size,
                &self.anchors,
                &mut quiet,
            );
            // Scrolled and clipped as it was last shown: scroll offsets outlive the list.
            present(&mut b, &mut self.presentation, None);
            let t = if self.style.motion_out > 0.0 {
                smooth((self.clock - l.start) / self.style.motion_out)
            } else {
                1.0
            };
            out.push((b, t));
        }
        out
    }
}

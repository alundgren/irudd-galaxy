//! Keys held down, and the inputs their changes make. The play window's keyboard and
//! right mouse button press keys here, and so does `galaxy-gm live input`, so a key held
//! from either one sends the same events: each event a module binds to the key goes down
//! once when the first hand takes the key and up once when the last one lets go.
//!
//! Each hand is tracked apart. The window letting go of its keys (it lost focus, a note
//! opened) leaves keys a live connection holds down, and a live connection closing lets
//! go of only its own.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_module::module::Module;
use serde_json::{Map, Value};

use crate::session::Input;

/// Manifest key names that are mouse buttons: their inputs have source `mouse`.
pub const MOUSE_KEYS: [&str; 1] = ["mouse_right"];

/// What [`Input::via`] says for an input a live connection made.
pub const VIA_LIVE: &str = "live";

/// Who holds a key down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Hand {
    /// The window's own keyboard and mouse.
    Window,
    /// A `galaxy-gm live` connection, by its number.
    Live(u64),
}

/// The keys the modules bind and the keys held now.
#[derive(Clone, Debug, Default)]
pub struct Keys {
    bindings: BTreeMap<String, Vec<String>>,
    held: BTreeMap<String, BTreeSet<Hand>>,
}

impl Keys {
    pub fn new<'a>(modules: impl Iterator<Item = &'a Module>) -> Keys {
        let mut keys = Keys::default();
        keys.rebind(modules);
        keys
    }

    /// Takes the bindings from `modules` (after a hot-load). Held keys stay held.
    pub fn rebind<'a>(&mut self, modules: impl Iterator<Item = &'a Module>) {
        self.bindings = bindings(modules);
    }

    /// Which events each key name sends, from every module's `[keys]`.
    pub fn bindings(&self) -> &BTreeMap<String, Vec<String>> {
        &self.bindings
    }

    /// Whether any module binds `key`.
    pub fn bound(&self, key: &str) -> bool {
        self.bindings.contains_key(key)
    }

    /// Whether any hand holds `key` down.
    pub fn is_held(&self, key: &str) -> bool {
        self.held.contains_key(key)
    }

    /// `hand` puts `key` down or lets it go: the inputs that makes. None unless whether
    /// any hand holds the key changed.
    pub fn press(&mut self, key: &str, down: bool, hand: Hand) -> Vec<Input> {
        let was = self.is_held(key);
        if down {
            self.held.entry(key.to_owned()).or_default().insert(hand);
        } else if let Some(hands) = self.held.get_mut(key) {
            hands.remove(&hand);
            if hands.is_empty() {
                self.held.remove(key);
            }
        }
        if was == self.is_held(key) {
            return Vec::new();
        }
        self.inputs(key, down, hand)
    }

    /// `hand` lets go of every key it holds: the inputs that makes.
    pub fn release(&mut self, hand: Hand) -> Vec<Input> {
        let mine: Vec<String> = self
            .held
            .iter()
            .filter(|(_, hands)| hands.contains(&hand))
            .map(|(k, _)| k.clone())
            .collect();
        mine.iter()
            .flat_map(|k| self.press(k, false, hand))
            .collect()
    }

    /// Every hand lets go of every key: the inputs that makes. A key the window held goes
    /// up as the window's; one only live connections held, as theirs.
    pub fn release_all(&mut self) -> Vec<Input> {
        let held = std::mem::take(&mut self.held);
        held.into_iter()
            .flat_map(|(key, hands)| {
                let hand = hands.into_iter().next().unwrap_or(Hand::Window);
                self.inputs(&key, false, hand)
            })
            .collect()
    }

    fn inputs(&self, key: &str, down: bool, hand: Hand) -> Vec<Input> {
        self.bindings
            .get(key)
            .into_iter()
            .flatten()
            .map(|event| {
                let mut input = key_input(key, event, down);
                if let Hand::Live(_) = hand {
                    input.via = Some(VIA_LIVE.to_owned());
                }
                input
            })
            .collect()
    }
}

/// Which events each key name sends, from every module's `[keys]`.
pub fn bindings<'a>(modules: impl Iterator<Item = &'a Module>) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in modules {
        for (key, event) in &m.manifest.keys {
            out.entry(key.clone())
                .or_default()
                .push(format!("{}.{event}", m.id()));
        }
    }
    out
}

/// The input a bound key or mouse button sends: source `mouse` for a button, else
/// `keyboard`; payload `{down}`.
pub fn key_input(key: &str, event: &str, down: bool) -> Input {
    let mut payload = Map::new();
    payload.insert("down".into(), Value::Bool(down));
    Input {
        tick: 0,
        source: if MOUSE_KEYS.contains(&key) {
            "mouse"
        } else {
            "keyboard"
        }
        .into(),
        name: event.to_owned(),
        payload,
        via: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Keys {
        let mut k = Keys::default();
        k.bindings.insert("w".into(), vec!["demo.ahead".into()]);
        k.bindings.insert("a".into(), vec!["demo.left".into()]);
        k
    }

    fn shown(inputs: &[Input]) -> Vec<(String, bool, Option<String>)> {
        inputs
            .iter()
            .map(|i| (i.name.clone(), i.payload["down"] == true, i.via.clone()))
            .collect()
    }

    #[test]
    fn a_key_sends_once_whichever_hands_hold_it() {
        let mut k = keys();
        let live = Hand::Live(3);
        assert_eq!(
            shown(&k.press("w", true, live)),
            [("demo.ahead".into(), true, Some("live".into()))]
        );
        assert!(k.press("w", true, Hand::Window).is_empty());
        // The window losing focus leaves the live hold alone.
        assert!(k.release(Hand::Window).is_empty());
        assert!(k.is_held("w"));
        assert_eq!(
            shown(&k.release(live)),
            [("demo.ahead".into(), false, Some("live".into()))]
        );
        assert!(!k.is_held("w"));
        // Unbound keys are held but send nothing.
        assert!(k.press("q", true, Hand::Window).is_empty());
        assert!(k.is_held("q"));
    }

    #[test]
    fn keys_held_together_go_up_apart() {
        let mut k = keys();
        let live = Hand::Live(1);
        k.press("w", true, live);
        k.press("a", true, live);
        assert_eq!(
            shown(&k.press("a", false, live)),
            [("demo.left".into(), false, Some("live".into()))]
        );
        assert!(k.is_held("w"));
        let window = k.press("w", true, Hand::Window);
        assert!(window.is_empty());
        // Held by both, it goes up as the window's.
        assert_eq!(
            shown(&k.release_all()),
            [("demo.ahead".into(), false, None)]
        );
        assert!(!k.is_held("w"));
    }
}

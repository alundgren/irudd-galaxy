//! Keys and clicks to [`PlayerInput`].
//!
//! The window maps winit's key codes onto [`Key`] so this stays plain and testable.
//! WASD thrusts screen-relative (the camera looks straight down the world's Z with +Y
//! up the screen, so W is +Y) and the sim stops the ship when they're released; docked,
//! flying off undocks. Space is the context action, held while mining. Clicks become click targets on the play
//! plane; the sim ignores them until the control flags unlock click controls.

use std::collections::BTreeSet;

use galaxy_contract::prelude::*;
use render::Camera;
use render::glam::Vec2 as Screen;

/// The keys the game reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    W,
    A,
    S,
    D,
    Space,
    T,
    Shift,
    /// The map.
    M,
    /// Pause.
    P,
    Escape,
    /// Zoom (also the mouse wheel).
    ZoomOut,
    ZoomIn,
    /// The number keys 1 to 9: pick that answer at the dock (0-based).
    Answer(u8),
}

/// Which keys are down, plus a Space press the ticks haven't seen yet.
///
/// Ticks run at 30 Hz and key events arrive between them, so a tap can go down and up
/// before the next tick. The press is latched and delivered to the next tick anyway
/// ([`Keys::tick_input`]). The OS's auto-repeat never counts: a key held through a
/// pause or a focus loss (both [`Keys::clear`]) stays let go until it's really pressed
/// again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keys {
    down: BTreeSet<Key>,
    /// A fresh Space press no tick has seen yet.
    action_press: bool,
    tag_press: bool,
    /// The action the last tick was given.
    action_sent: bool,
    /// Flight keys were held into the docking and haven't all been let go since: they
    /// don't fly off. Keys still held from the approach never fly straight back out.
    held_into_dock: bool,
}

impl Keys {
    /// A key event from the window. Returns true for a fresh press: one that isn't
    /// auto-repeat and isn't of a key already down.
    pub fn event(&mut self, key: Key, down: bool, repeat: bool) -> bool {
        if repeat {
            return false;
        }
        if !down {
            self.down.remove(&key);
            return false;
        }
        let fresh = self.down.insert(key);
        if fresh && key == Key::Space {
            self.action_press = true;
        }
        if fresh && key == Key::T {
            self.tag_press = true;
        }
        fresh
    }

    pub fn set(&mut self, key: Key, down: bool) {
        self.event(key, down, false);
    }

    pub fn press(&mut self, key: Key) {
        self.set(key, true);
    }

    pub fn release(&mut self, key: Key) {
        self.set(key, false);
    }

    /// A press of the context action that isn't a key (scripts and tests).
    pub fn press_action(&mut self) {
        self.action_press = true;
    }

    /// Lets go of everything and forgets any press not yet delivered: the window lost
    /// focus, or the PAUSED card opened or closed. Keys still physically down then need
    /// a real release and press to count again.
    pub fn clear(&mut self) {
        self.down.clear();
        self.action_press = false;
        self.tag_press = false;
    }

    /// The context action for the next tick. A press since the last tick reaches it
    /// even if the key was already let go. If the last tick had it held (so the key was
    /// let go and pressed again in between), this tick sees it let go and the next one
    /// sees the press.
    pub fn take_action(&mut self) -> bool {
        let action = if self.action_press && self.action_sent {
            false
        } else {
            let pressed = std::mem::take(&mut self.action_press);
            pressed || self.held(Key::Space)
        };
        self.action_sent = action;
        action
    }

    /// The next tick's input, taking any latched press ([`Keys::take_action`]). Docked,
    /// thrust flies off and undocks, but only once every flight key has been let go
    /// since the docking.
    pub fn tick_input(&mut self, docked: bool, click: Option<Click>) -> PlayerInput {
        let action = self.take_action();
        let mut input = PlayerInput {
            action,
            tag: std::mem::take(&mut self.tag_press),
            ..player_input(self, click)
        };
        if !docked || input.thrust == Vec2::ZERO {
            self.held_into_dock = input.thrust != Vec2::ZERO;
        } else if self.held_into_dock {
            input.thrust = Vec2::ZERO;
        }
        input
    }

    pub fn held(&self, key: Key) -> bool {
        self.down.contains(&key)
    }

    fn axis(&self, minus: Key, plus: Key) -> f32 {
        f32::from(u8::from(self.held(plus))) - f32::from(u8::from(self.held(minus)))
    }
}

/// WASD as a thrust vector of length 0 or 1. Opposite keys cancel.
pub fn thrust(keys: &Keys) -> Vec2 {
    let v = Vec2::new(keys.axis(Key::A, Key::D), keys.axis(Key::S, Key::W));
    v.normalize_or_zero()
}

/// The input the held keys make. Docked, thrust flies off and undocks (Space does
/// nothing there). `click` is consumed by the first tick it goes into. The window and
/// scripts use [`Keys::tick_input`], which adds latched presses and keeps keys held
/// from the approach from undocking.
pub fn player_input(keys: &Keys, click: Option<Click>) -> PlayerInput {
    PlayerInput {
        thrust: thrust(keys),
        action: keys.held(Key::Space),
        tag: false,
        click,
        trades: Vec::new(),
        services: Vec::new(),
        replies: Vec::new(),
    }
}

/// What a click on the play plane at `at` points at: a station's docking ring, a rock,
/// or just a point.
pub fn click_target(world: &WorldState, region: &RegionId, at: Vec2) -> ClickTarget {
    if let Some(station) = galaxy_ship::space::ring_at(world, region, at) {
        return ClickTarget::Station {
            station: station.id.clone(),
        };
    }
    let rock = world
        .rocks
        .values()
        .filter(|r| r.remaining > 0)
        .filter(|r| galaxy_ship::space::rock_region(world, r) == Some(region))
        .map(|r| (r, r.position.distance(at)))
        .filter(|(r, d)| *d <= galaxy_ship::space::rock_radius(world, r))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((rock, _)) = rock {
        return ClickTarget::Rock {
            rock: rock.id.clone(),
        };
    }
    ClickTarget::Point { at }
}

/// A click at `screen` (logical pixels) seen through `camera`, as the sim's click. Shift
/// held queues it instead of replacing the current orders.
pub fn click_at(
    world: &WorldState,
    camera: &Camera,
    screen: Screen,
    viewport: Screen,
    queue: bool,
) -> Option<Click> {
    let at = plane_point(camera, screen, viewport)?;
    let ship = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))?;
    Some(Click {
        target: click_target(world, &ship.region, at),
        queue,
    })
}

/// The point on the play plane (`z = 0`) under `screen`.
pub fn plane_point(camera: &Camera, screen: Screen, viewport: Screen) -> Option<Vec2> {
    let p = camera.screen_to_plane(screen, viewport, 0.0)?;
    Some(Vec2::new(p.x, p.y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use render::glam::Vec3;

    #[test]
    fn wasd_thrusts_screen_relative_and_stops_on_release() {
        let mut k = Keys::default();
        assert_eq!(thrust(&k), Vec2::ZERO);
        k.press(Key::W);
        assert_eq!(thrust(&k), Vec2::new(0.0, 1.0));
        k.press(Key::D);
        let t = thrust(&k);
        assert!((t.length() - 1.0).abs() < 1e-5 && t.x > 0.0 && t.y > 0.0);
        k.press(Key::S);
        assert_eq!(thrust(&k), Vec2::new(1.0, 0.0), "W and S cancel");
        k.release(Key::W);
        k.release(Key::S);
        k.release(Key::D);
        k.press(Key::A);
        assert_eq!(thrust(&k), Vec2::new(-1.0, 0.0));
        k.release(Key::A);
        let input = player_input(&k, None);
        assert_eq!(input, PlayerInput::default(), "released keys are hands off");
    }

    #[test]
    fn space_is_the_held_context_action() {
        let mut k = Keys::default();
        k.press(Key::Space);
        assert!(player_input(&k, None).action);
        k.release(Key::Space);
        assert!(!player_input(&k, None).action);
    }

    #[test]
    fn a_tap_between_ticks_reaches_the_next_tick_once() {
        let mut k = Keys::default();
        assert!(k.event(Key::Space, true, false));
        assert!(!k.event(Key::Space, false, false));
        assert!(k.tick_input(true, None).action, "the tap is latched");
        assert!(!k.tick_input(true, None).action, "and delivered once");
    }

    #[test]
    fn a_quick_release_and_repress_reads_as_a_new_press() {
        let mut k = Keys::default();
        k.press(Key::Space);
        assert!(k.take_action());
        // Let go and pressed again before the next tick.
        k.release(Key::Space);
        k.press(Key::Space);
        assert!(!k.take_action(), "this tick sees the release");
        assert!(k.take_action(), "the next sees the press");
        assert!(k.take_action(), "then it's held");
    }

    #[test]
    fn auto_repeat_and_keys_held_through_a_clear_dont_count() {
        let mut k = Keys::default();
        k.press(Key::Space);
        assert!(k.take_action());
        // The PAUSED card opens (or focus goes) with Space still down, and closes.
        k.clear();
        assert!(!k.event(Key::Space, true, true), "repeat isn't a press");
        assert!(!k.held(Key::Space));
        assert!(!k.take_action());
        assert!(!k.event(Key::W, true, true));
        assert_eq!(k.tick_input(false, None).thrust, Vec2::ZERO);
        // A real release and press does.
        k.event(Key::Space, false, false);
        assert!(k.event(Key::Space, true, false));
        assert!(k.take_action());
    }

    #[test]
    fn a_press_latched_before_a_clear_is_forgotten() {
        let mut k = Keys::default();
        k.press(Key::Space);
        k.release(Key::Space);
        k.clear();
        assert!(!k.take_action());
        k.press_action();
        assert!(k.take_action(), "a scripted press");
        assert!(!k.take_action());
    }

    #[test]
    fn docked_a_fresh_thrust_flies_off() {
        let mut k = Keys::default();
        // Docked with no keys down: W flies off.
        assert_eq!(k.tick_input(true, None).thrust, Vec2::ZERO);
        k.press(Key::W);
        assert_eq!(k.tick_input(true, None).thrust, Vec2::new(0.0, 1.0));
    }

    #[test]
    fn keys_held_from_the_approach_dont_fly_straight_back_out() {
        let mut k = Keys::default();
        k.press(Key::D);
        assert_eq!(k.tick_input(false, None).thrust, Vec2::new(1.0, 0.0));
        // Space in the ring docks with D still down: docked, D does nothing...
        assert_eq!(k.tick_input(true, None).thrust, Vec2::ZERO);
        assert_eq!(k.tick_input(true, None).thrust, Vec2::ZERO);
        // ...until it's let go and pressed again.
        k.release(Key::D);
        assert_eq!(k.tick_input(true, None).thrust, Vec2::ZERO);
        k.press(Key::A);
        assert_eq!(k.tick_input(true, None).thrust, Vec2::new(-1.0, 0.0));
        // Undocked, the next docking starts over.
        assert_eq!(k.tick_input(false, None).thrust, Vec2::new(-1.0, 0.0));
        assert_eq!(k.tick_input(true, None).thrust, Vec2::ZERO);
    }

    #[test]
    fn clicks_land_on_the_plane() {
        let viewport = Screen::new(1280.0, 720.0);
        let cam = Camera::top_down(Screen::new(100.0, -50.0), 400.0);
        // The middle of the screen is the camera centre; the top edge is half a view up.
        let mid = plane_point(&cam, viewport * 0.5, viewport).unwrap();
        assert!(
            (mid.x - 100.0).abs() < 0.01 && (mid.y + 50.0).abs() < 0.01,
            "{mid:?}"
        );
        let top = plane_point(&cam, Screen::new(640.0, 0.0), viewport).unwrap();
        assert!((top.y - 150.0).abs() < 0.5, "{top:?}");
        // Tilted, a point drawn on screen maps back onto itself.
        let tilted = Camera::top_down(Screen::new(0.0, 0.0), 400.0).with_tilt(14.0);
        let world = Vec3::new(37.0, -12.0, 0.0);
        let s = tilted.world_to_screen(world, viewport).unwrap();
        let back = plane_point(&tilted, s, viewport).unwrap();
        assert!(
            (back.x - 37.0).abs() < 0.05 && (back.y + 12.0).abs() < 0.05,
            "{back:?}"
        );
    }
}

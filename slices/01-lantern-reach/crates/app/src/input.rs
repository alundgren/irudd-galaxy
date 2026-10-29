//! Keys and clicks to [`PlayerInput`].
//!
//! The window maps winit's key codes onto [`Key`] so this stays plain and testable.
//! WASD thrusts screen-relative (the camera looks straight down the world's Z with +Y
//! up the screen, so W is +Y) and the sim stops the ship when they're released; Space
//! is the context action, held while mining. Clicks become click targets on the play
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
    Up,
    Down,
    Left,
    Right,
    Space,
    Shift,
    /// Trade the selected dock line.
    E,
    Enter,
    /// The map.
    M,
    /// Pause.
    P,
    Escape,
    /// Zoom (also the mouse wheel).
    ZoomOut,
    ZoomIn,
}

/// Which keys are down.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keys {
    down: BTreeSet<Key>,
}

impl Keys {
    pub fn set(&mut self, key: Key, down: bool) {
        if down {
            self.down.insert(key);
        } else {
            self.down.remove(&key);
        }
    }

    pub fn press(&mut self, key: Key) {
        self.set(key, true);
    }

    pub fn release(&mut self, key: Key) {
        self.set(key, false);
    }

    /// Lets go of everything (the window lost focus).
    pub fn clear(&mut self) {
        self.down.clear();
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

/// This tick's input. While docked the keys drive the dock screen, so there is no
/// thrust (Space still undocks). `click` is consumed by the first tick it goes into.
pub fn player_input(keys: &Keys, docked: bool, click: Option<Click>) -> PlayerInput {
    PlayerInput {
        thrust: if docked { Vec2::ZERO } else { thrust(keys) },
        action: keys.held(Key::Space),
        click,
        trades: Vec::new(),
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
        let input = player_input(&k, false, None);
        assert_eq!(input, PlayerInput::default(), "released keys are hands off");
    }

    #[test]
    fn space_is_the_held_context_action() {
        let mut k = Keys::default();
        k.press(Key::Space);
        assert!(player_input(&k, false, None).action);
        assert!(player_input(&k, true, None).action, "Space undocks");
        k.release(Key::Space);
        assert!(!player_input(&k, false, None).action);
    }

    #[test]
    fn docked_keys_drive_the_dock_screen_not_the_ship() {
        let mut k = Keys::default();
        k.press(Key::W);
        assert_eq!(player_input(&k, true, None).thrust, Vec2::ZERO);
        assert_eq!(player_input(&k, false, None).thrust, Vec2::new(0.0, 1.0));
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

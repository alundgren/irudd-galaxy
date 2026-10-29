//! One frame: the world (or the map) plus the screen's panels. The window and
//! `galaxy shot` build frames the same way, so a shot shows exactly what play shows.

use render::Scene;
use render::glam::Vec2 as G2;

use crate::session::Session;
use crate::ui::{self, Button, UiState};
use crate::view::{CameraRig, View, g};

/// Where the camera should look: the flagship, drawn between ticks. Docked, the station
/// sits in the middle of the space the dock screen leaves free.
pub fn camera_target(session: &Session, rig: &CameraRig, alpha: f32, viewport: G2) -> Option<G2> {
    let ship = session.flagship()?;
    if let Some(st) = ship
        .docked_at
        .as_ref()
        .and_then(|s| session.world().stations.get(s))
    {
        let free = viewport.x - ui::DOCK_PANEL_WIDTH - 18.0;
        let shift_px = viewport.x * 0.5 - free * 0.5;
        let wpp = rig.camera().world_per_pixel(viewport);
        return Some(g(st.position) + G2::new(shift_px * wpp, 0.0));
    }
    let poses = session.poses(alpha);
    Some(
        poses
            .get(&ship.id)
            .map_or(g(ship.position), |p| g(p.position)),
    )
}

/// Builds the frame's scene and the buttons on it. `alpha` is how far into the next
/// tick real time is (for interpolation); `time` is presentation seconds.
pub fn compose(
    view: &View,
    session: &Session,
    rig: &CameraRig,
    ui_state: &UiState,
    alpha: f32,
    time: f32,
    viewport: G2,
) -> (Scene, Vec<Button>) {
    let world = session.world();
    let mut buttons = Vec::new();
    let mut scene = if ui_state.map_open {
        let mut s = view.map_scene(world, time, viewport);
        buttons.extend(ui::map_furniture(&mut s.overlay, ui_state, viewport));
        s
    } else {
        let poses = session.poses(alpha);
        let mut s = view.world_scene(world, &poses, rig, time, viewport);
        let camera = rig.camera();
        ui::station_labels(&mut s.overlay, world, &camera, viewport);
        ui::edge_markers(&mut s.overlay, world, &camera, viewport);
        ui::hint(&mut s.overlay, world, viewport);
        ui::talk(&mut s.overlay, world, viewport);
        if let Some(station) = session.flagship().and_then(|f| f.docked_at.clone()) {
            buttons.extend(ui::dock_panel(
                &mut s.overlay,
                world,
                &station,
                ui_state,
                session.trades.notice.as_deref(),
                viewport,
            ));
        }
        s
    };
    ui::goal(&mut scene.overlay, world, viewport);
    ui::hold(&mut scene.overlay, world, viewport);
    if session.is_paused() {
        scene.overlay.paused = Some(ui::paused_card());
        // Nothing is clickable under the card.
        buttons.clear();
    }
    (scene, buttons)
}

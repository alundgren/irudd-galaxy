//! One frame: the world (or the map) plus the screen's panels. The window and
//! `galaxy shot` build frames the same way, so a shot shows exactly what play shows.

use render::glam::Vec2 as G2;
use render::{Overlay, Scene};

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
        let panel = (ui::DOCK_PANEL_WIDTH + 18.0) * ui::ui_scale(viewport);
        let free = viewport.x - panel;
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
///
/// Labels pinned to the world use the window's pixels; the panels (and the map's
/// labels) are laid out on the UI canvas and scaled onto the window (see [`ui`]),
/// buttons included.
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
    let history = session.sim().history();
    let scale = ui::ui_scale(viewport);
    let canvas = ui::canvas(viewport);
    let on_canvas = UiState {
        cursor: ui_state.cursor.map(|c| c / scale),
        ..ui_state.clone()
    };
    let mut panels = Overlay::default();
    let mut buttons = Vec::new();
    let mut scene = if ui_state.map_open {
        // The map's camera depends only on the window's shape, so its labels and cards
        // can be laid out on the canvas too and scaled with the panels.
        let mut s = view.map_scene(world, time, canvas);
        ui::scale_overlay(&mut s.overlay, scale);
        buttons.extend(ui::map_furniture(&mut panels, &on_canvas, canvas));
        s
    } else {
        let poses = session.poses(alpha);
        let mut s = view.world_scene(world, &poses, rig, time, viewport);
        let camera = rig.camera();
        ui::station_labels(&mut s.overlay, world, &camera, viewport);
        ui::edge_markers(&mut s.overlay, world, &camera, viewport);
        ui::hint(&mut panels, world, canvas);
        let docked = session.flagship().and_then(|f| f.docked_at.clone());
        let answer = session
            .answer
            .as_ref()
            .filter(|a| docked.as_ref() == Some(&a.station));
        let choices = docked
            .as_ref()
            .map(|st| crate::hooks::open_choices(world, history, st))
            .unwrap_or_default();
        buttons.extend(ui::talk(
            &mut panels,
            world,
            history,
            answer,
            &choices,
            &on_canvas,
            canvas,
        ));
        if let Some(station) = docked {
            buttons.extend(ui::dock_panel(
                &mut panels,
                world,
                history,
                &station,
                &on_canvas,
                ui::Notices {
                    trade: session.trades.notice.as_deref(),
                    service: session.services.notice.as_deref(),
                },
                canvas,
            ));
        }
        s
    };
    ui::goal(&mut panels, world, canvas);
    ui::hold(&mut panels, world, canvas);
    if let Some(to) = &ui_state.confirm
        && session.flagship().is_some_and(|f| f.docked_at.is_some())
    {
        // The card is on top: only its own buttons work while it's open.
        buttons = ui::prestige_card(&mut panels, world, history, to, &on_canvas, canvas);
    }
    if ui_state.debug_open {
        ui::debug_overlay(&mut panels, session);
    }
    ui::scale_overlay(&mut panels, scale);
    ui::scale_buttons(&mut buttons, scale);
    // Labels and markers pinned to the world never show through a panel.
    let under = |p: G2| {
        panels.panels.iter().any(|q| {
            let r = q.rect;
            p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h
        })
    };
    scene.overlay.texts.retain(|t| !under(t.pos));
    scene.overlay.panels.retain(|q| !under(q.rect.center()));
    scene.overlay.panels.extend(panels.panels);
    scene.overlay.texts.extend(panels.texts);
    if session.is_paused() {
        scene.overlay.paused = Some(ui::paused_card());
        // Nothing is clickable under the card.
        buttons.clear();
    }
    (scene, buttons)
}

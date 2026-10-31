//! One frame: the world (or the map) plus the screen's panels. The window and
//! `galaxy shot` build frames the same way, so a shot shows exactly what play shows.

use std::borrow::Cow;

use galaxy_contract::prelude::*;
use render::glam::Vec2 as G2;
use render::{Camera, Overlay, Rect, Scene};

use crate::session::Session;
use crate::ui::{self, Button, UiState};
use crate::view::{CameraRig, TILT, View, g};

/// Where the camera should be: what it looks at, and docked, how much it sees (`None`:
/// the flight zoom from the data).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraGoal {
    pub center: G2,
    pub zoom: Option<f32>,
}

/// The share of the screen's height the whole station fills when docked.
const DOCK_FILL: f32 = 0.6;
/// How far right of the middle the whole station sits, as a share of the width.
const OVERVIEW_SHIFT: f32 = 0.07;
/// The narrowest room left of the station that bubbles keep to.
const BUBBLE_MIN_W: f32 = 340.0;
/// How much closer the camera comes when a room opens.
const ROOM_ZOOM: f32 = 0.85;

/// The room open at the dock now: the part (or stand-in room,
/// [`ui::station::stand_ins`]), if the flagship is docked at its station.
pub fn open_room<'a>(session: &'a Session, ui_state: &UiState) -> Option<Cow<'a, StationPart>> {
    let docked = session.flagship()?.docked_at.as_ref()?;
    let id = ui_state.room.as_ref()?;
    ui::station::room_here(session.world(), docked, id)
}

/// The open room if it's an open wharf the fit rules know: the hullwright's cradle
/// (#47, 3.2), with the flagship lifted into it.
pub fn cradle_room<'a>(
    session: &'a Session,
    ui_state: &UiState,
) -> Option<(StationId, Cow<'a, StationPart>)> {
    let part = open_room(session, ui_state)?;
    let station = session.flagship()?.docked_at.clone()?;
    let world = session.world();
    (part.room == galaxy_contract::parts::RoomKind::Wharf
        && part.state == galaxy_contract::parts::PartState::Open
        && world.progress.rules.fit.wharves.contains_key(&station))
    .then_some((station, part))
}

/// What the cradle draws this frame, if a wharf's room is open.
pub fn cradle_spec(
    session: &Session,
    ui_state: &UiState,
) -> Option<crate::view::cradle::CradleSpec> {
    let (station, part) = cradle_room(session, ui_state)?;
    crate::view::cradle::spec(
        session.world(),
        session.sim().history(),
        &station,
        &part,
        &ui_state.wharf,
    )
}

/// Uploads what this frame needs that [`View::sync`] doesn't know about: the ship in the
/// cradle as it's drawn now. Call before [`compose`].
pub fn prepare(view: &mut View, r: &mut render::Renderer, session: &Session, ui_state: &UiState) {
    if let Some(spec) = cradle_spec(session, ui_state) {
        view.prepare_cradle(r, &spec);
    }
}

/// The camera's goal. Flying, the flagship, drawn between ticks. Docked, the camera
/// comes in close on the whole station (its art and every module); with a room open,
/// closer still, on the room's module in the middle of the space the room leaves free.
pub fn camera_goal(
    session: &Session,
    ui_state: &UiState,
    alpha: f32,
    viewport: G2,
) -> Option<CameraGoal> {
    let ship = session.flagship()?;
    let world = session.world();
    let Some(st) = ship.docked_at.as_ref().and_then(|s| world.stations.get(s)) else {
        let poses = session.poses(alpha);
        return Some(CameraGoal {
            center: poses
                .get(&ship.id)
                .map_or(g(ship.position), |p| g(p.position)),
            zoom: None,
        });
    };
    let recipe = galaxy_core::parts::recipe_of(world, st);
    let to_world = crate::view::station_transform(st, &recipe);
    let centre = to_world.w_axis.truncate().truncate();
    let mut reach = recipe.footprint() * to_world.x_axis.truncate().length();
    for p in ui::station::parts_here(world, &st.id) {
        if let Some((at, r)) = crate::view::modules::module_point(world, p) {
            reach = reach.max(at.distance(centre) + r);
        }
    }
    let zoom = (reach * 2.0 / DOCK_FILL).max(160.0);
    let Some(part) = open_room(session, ui_state) else {
        // A little right of the middle: talk bubbles open on the left.
        let wpp = Camera::top_down(centre, zoom)
            .with_tilt(TILT)
            .world_per_pixel(viewport);
        return Some(CameraGoal {
            center: centre - G2::new(viewport.x * OVERVIEW_SHIFT * wpp, 0.0),
            zoom: Some(zoom),
        });
    };
    let canvas = ui::canvas(viewport);
    if let Some((_, wharf)) = cradle_room(session, ui_state) {
        // The cradle, close up, in the room the panel leaves on its left.
        let free = ui::station::bubble_bounds(world, &st.id, true, session.talk.log_new(), canvas);
        if let Some((center, zoom)) = crate::view::cradle::camera_goal(world, &wharf, free, canvas)
        {
            return Some(CameraGoal {
                center,
                zoom: Some(zoom),
            });
        }
    }
    let zoom = zoom * ROOM_ZOOM;
    let at = crate::view::modules::module_point(world, &part).map_or(centre, |(p, _)| p);
    // The module in the middle of what the room leaves free on its left.
    let free = ui::station::room_rect(world, &st.id, canvas).x * ui::ui_scale(viewport);
    let shift_px = viewport.x * 0.5 - free * 0.5;
    let wpp = Camera::top_down(at, zoom)
        .with_tilt(TILT)
        .world_per_pixel(viewport);
    Some(CameraGoal {
        center: at + G2::new(shift_px * wpp, 0.0),
        zoom: Some(zoom),
    })
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
    // The open log pins every job with who gave it, so the banner steps aside for it.
    let log_open = ui_state.log_open && !ui_state.map_open && ui_state.confirm.is_none();
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
        ui::edge_markers(
            &mut s.overlay,
            world,
            &camera,
            viewport,
            ui_state.goal_marker,
        );
        let docked = session.flagship().and_then(|f| f.docked_at.clone());
        let log_new = session.talk.log_new();
        // The hint pill keeps clear of the log icon in the bottom-left corner.
        ui::hint(
            &mut panels,
            world,
            &session.hints,
            Some(ui::log::icon_area(log_new, canvas)),
            canvas,
        );
        let mut hovered = None;
        // The prestige card is modal. Every text draws after every panel, so whatever
        // sat under the card would print over it: leave the station frame and talk out.
        if ui_state.confirm.is_none()
            && let Some(station) = docked
        {
            hovered = station_frame(
                &mut panels,
                &mut buttons,
                session,
                &on_canvas,
                &camera,
                &station,
                log_open,
                (viewport, canvas, scale),
            );
        }
        view.draw_modules(&mut s, world, rig, hovered.as_ref());
        if ui_state.confirm.is_none()
            && let Some(spec) = cradle_spec(session, ui_state)
        {
            view.draw_cradle(&mut s, &spec, viewport);
        }
        s
    };
    if ui_state.map_open {
        ui::map_goal(&mut panels, world, canvas);
    } else if !log_open {
        ui::goal(&mut panels, world, canvas);
    }
    ui::hold(&mut panels, world, canvas);
    // The ship log: its icon bottom left, and the panel over the left column when open.
    if !ui_state.map_open && ui_state.confirm.is_none() {
        buttons.extend(ui::log::icon(
            &mut panels,
            session.talk.log_new(),
            ui_state.log_open,
            &on_canvas,
            canvas,
        ));
        if log_open {
            buttons.extend(ui::log::panel(
                &mut panels,
                world,
                history,
                &on_canvas,
                canvas,
            ));
        }
    }
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

/// The station frame at the dock: module targets and signs, the plate, the station key,
/// the open room and the talk bubble rising from its speaker's module. Returns the
/// hovered part (by `ui_state.hover`, else the mouse over a key icon or a module).
#[allow(clippy::too_many_arguments)]
fn station_frame(
    panels: &mut Overlay,
    buttons: &mut Vec<Button>,
    session: &Session,
    ui_state: &UiState,
    camera: &Camera,
    station: &StationId,
    log_open: bool,
    (viewport, canvas, scale): (G2, G2, f32),
) -> Option<StationPartId> {
    let world = session.world();
    let log_new = session.talk.log_new();
    let room = open_room(session, ui_state);
    let room_rect = room
        .as_ref()
        .map(|_| ui::station::room_rect(world, station, canvas));
    let key = ui::station::key_rects(world, station, canvas);
    let key_area = ui::station::key_area(world, station, canvas);
    let plate = ui::station::plate_rect(world, station);
    let cradle = cradle_room(session, ui_state);
    let mut cover: Vec<Rect> = vec![plate, ui::log::icon_area(log_new, canvas)];
    cover.extend(key_area);
    cover.extend(room_rect);
    if let Some((_, part)) = &cradle {
        cover.extend(crate::view::cradle::canvas_rect(
            world, part, camera, viewport, scale,
        ));
    }
    cover.extend(ui::hold_rect(world, canvas));
    if log_open {
        cover.push(ui::log::panel_rect(canvas));
    } else {
        cover.extend(ui::goal_rect(world, canvas));
    }
    let targets: Vec<ui::station::Target> =
        ui::station::module_targets(world, station, camera, viewport, scale)
            .into_iter()
            .filter(|t| !cover.iter().any(|r| ui::contains(r, t.at)))
            .collect();
    let cursor = ui_state.cursor;
    let hovered = ui_state
        .hover
        .clone()
        .or_else(|| {
            let c = cursor?;
            key.iter()
                .find(|(_, r)| ui::contains(r, c))
                .map(|(id, _)| id.clone())
        })
        .or_else(|| {
            let c = cursor.filter(|c| !cover.iter().any(|r| ui::contains(r, *c)))?;
            ui::station::module_at(&targets, c).cloned()
        });
    // Modules first: every panel's buttons sit on top of them.
    buttons.extend(ui::station::module_buttons(&targets, canvas));
    // The bubble, laid out first so no sign prints through it.
    let bubble = (!log_open).then(|| session.bubble()).flatten().map(|b| {
        let mut bounds =
            ui::station::bubble_bounds(world, station, room.is_some(), log_new, canvas);
        // Over the whole station, bubbles keep to the space left of it when that's
        // roomy enough, so every module stays in view.
        if room.is_none()
            && let Some(left) = ui::station::art_left(world, station, camera, viewport, scale)
            && left - ui::GAP - bounds.x >= BUBBLE_MIN_W
        {
            bounds.w = bounds.w.min(left - ui::GAP - bounds.x);
        }
        let anchor = session
            .anchors
            .anchor(world, &b.line, station)
            .and_then(|at| camera.world_to_screen(g(at).extend(0.0), viewport))
            .map_or(bounds.center(), |p| p / scale);
        let rect = ui::talk::layout(&b, world, anchor, bounds).rect;
        (b, anchor, bounds, rect)
    });
    let mut avoid = cover.clone();
    avoid.extend(bubble.as_ref().map(|(.., r)| *r));
    if let (Some(area), true) = (
        key_area,
        key.iter().any(|(id, _)| hovered.as_ref() == Some(id)),
    ) {
        // The hovered icon's headline above the key.
        avoid.push(Rect::new(0.0, area.y - 80.0, canvas.x, 80.0));
    }
    ui::station::signs(panels, world, &targets, hovered.as_ref(), &avoid, canvas);
    if !log_open {
        ui::station::plate(panels, world, station);
    }
    buttons.extend(ui::station::key(
        panels,
        world,
        station,
        ui_state,
        hovered.as_ref(),
        canvas,
    ));
    if let (Some(part), Some(rect)) = (room, room_rect) {
        let r = ui::rooms::Room {
            world,
            history: session.sim().history(),
            station,
            part: &part,
            ui: ui_state,
            notices: ui::Notices {
                trade: session.trades.notice.as_deref(),
                service: session.services.notice.as_deref(),
                fitted: session.services.fitted.as_deref(),
            },
        };
        if cradle.is_some() {
            let free = ui::station::bubble_bounds(world, station, true, log_new, canvas);
            buttons.extend(ui::rooms::cradle::furniture(
                panels, &r, camera, viewport, scale, free,
            ));
        }
        buttons.extend(ui::rooms::draw(panels, &r, rect));
    }
    if let Some((b, anchor, bounds, _)) = bubble {
        let (_, bubble) = ui::talk::dock_bubble(panels, world, &b, anchor, bounds, ui_state);
        buttons.extend(bubble);
    }
    hovered
}

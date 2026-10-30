//! `galaxy shot`: a save rendered headless to a PNG with the same scene mapping and
//! panels as the window, optionally after a scripted input run ([`crate::script`]).

use std::path::PathBuf;

use galaxy_core::Save;
use render::Renderer;
use render::glam::Vec2 as G2;
use serde_json::{Value, json};

use crate::frame;
use crate::session::Session;
use crate::ui::UiState;
use crate::view::{CameraRig, View};

#[derive(Clone, Debug)]
pub struct ShotArgs {
    pub save: PathBuf,
    pub out: PathBuf,
    pub map: bool,
    pub width: u32,
    pub height: u32,
    /// Visible world height; default the game's.
    pub zoom: Option<f32>,
    /// Scripted input to run first.
    pub keys: Option<String>,
    /// Write the scripted run back to the save.
    pub write: bool,
}

/// Parses `WxH`.
pub fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("size `{s}` should look like 1280x720"))?;
    let w: u32 = w
        .trim()
        .parse()
        .map_err(|_| format!("bad width in `{s}`"))?;
    let h: u32 = h
        .trim()
        .parse()
        .map_err(|_| format!("bad height in `{s}`"))?;
    if !(64..=8192).contains(&w) || !(64..=8192).contains(&h) {
        return Err(format!("size `{s}` is out of range (64..8192)"));
    }
    Ok((w, h))
}

/// Errors: (exit code, message). 4 for bad input, 1 for rendering or I/O.
pub fn shot(args: &ShotArgs) -> Result<Value, (u8, String)> {
    let mut save = Save::open(&args.save).map_err(|e| (4, e.to_string()))?;
    let state = save.load().map_err(|e| (4, e.to_string()))?;
    let packs = crate::game::packs_dir(None).ok_or((
        4,
        "can't find the packs folder; set GALAXY_PACKS".to_owned(),
    ))?;
    let seed = state.world.meta.seed;
    // The same composition as play: the game master's applied packs run in the game's
    // script host too (staged ones wait for a real load).
    let (game, _bridge) = crate::game::Game::open_gm(
        state,
        &crate::game::GameOptions::headless(packs, seed),
        &args.save,
        galaxy_gm::BridgeOptions::default(),
    )
    .map_err(|e| (4, e))?;
    let mut session = Session::with_game(game);
    let mut ui_state = UiState {
        map_open: args.map,
        ..UiState::default()
    };
    let mut ticks = 0;
    if let Some(script) = &args.keys {
        ticks = crate::script::run(&mut session, &mut ui_state, script).map_err(|e| (4, e))?;
        if args.write {
            session.finish();
            save.write(session.state())
                .map_err(|e| (1, e.to_string()))?;
        }
    }
    let mut renderer =
        Renderer::new_headless(args.width, args.height).map_err(|e| (1, e.to_string()))?;
    let mut view = View::new();
    view.sync(&mut renderer, session.world());
    let mut rig = CameraRig::new(G2::ZERO);
    if let Some(z) = args.zoom {
        rig.zoom = z.clamp(CameraRig::MIN_ZOOM, CameraRig::MAX_ZOOM * 4.0);
    }
    if let Some(target) = frame::camera_target(&session, &rig, 1.0, renderer.logical_size()) {
        rig.center = target;
    }
    let time = session.world().meta.tick.0 as f32 / galaxy_contract::TICKS_PER_SECOND as f32;
    let (scene, _) = frame::compose(
        &view,
        &session,
        &rig,
        &ui_state,
        1.0,
        time,
        renderer.logical_size(),
    );
    let image = renderer
        .render_to_rgba(&scene)
        .map_err(|e| (1, e.to_string()))?;
    image.save_png(&args.out).map_err(|e| (1, e.to_string()))?;
    let world = session.world();
    let ship = session.flagship();
    Ok(json!({
        "out": args.out,
        "width": image.width,
        "height": image.height,
        "tick": world.meta.tick,
        "ticks_run": ticks,
        "map": ui_state.map_open,
        "paused": session.is_paused(),
        "docked_at": ship.and_then(|s| s.docked_at.clone()),
        "position": ship.map(|s| s.position),
        "hold": ship.map(|s| s.hold.cargo.clone()),
        "credits": crate::trades::credits(world.player.credits),
        "goal": crate::hooks::goal_line(world),
        "notice": session.trades.notice,
        "service_notice": session.services.notice,
        "fitted": session.services.fitted,
        "talk": crate::hooks::talk_lines(world, session.sim().history(), 3)
            .iter()
            .map(|l| json!({
                "speaker": crate::hooks::speaker(world, l),
                "place": crate::hooks::place(world, l),
                "text": l.text,
            }))
            .collect::<Vec<_>>(),
        "written": args.write && args.keys.is_some(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse() {
        assert_eq!(parse_size("1280x720").unwrap(), (1280, 720));
        assert_eq!(parse_size("640X360").unwrap(), (640, 360));
        assert!(parse_size("12x720").is_err());
        assert!(parse_size("wide").is_err());
    }
}

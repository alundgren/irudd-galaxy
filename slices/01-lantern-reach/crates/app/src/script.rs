//! Scripted input for `galaxy shot --keys`: fly a save headless with the same input
//! mapping the window uses, so agents (and tests) can look at the game mid-play.
//!
//! A script is comma-separated steps, each `STEP` or `STEP*N` (N ticks, default 1):
//!
//! - keys joined by `+`, held for N ticks then released: `w`, `a`, `s`, `d`, `space`,
//!   `shift`, `w+d`, `shift+w`, `space*90` (tank controls: A/D turn, W/S thrust, Shift
//!   boosts);
//! - `wait*N`: hands off for N ticks;
//! - `:pause`, `:map`: toggle the PAUSED card or the map;
//! - `:next`, `:prev`: move the dock screen's selection;
//! - `:trade`, `:tradeall`: trade one, or all, of the selected dock line (then the
//!   ticks run so the trade settles);
//! - `:buy:ID`: buy a station service, as its dock-screen button does: an upgrade
//!   (`upgrade.mining_lens`), a tender (`craft.thimble`) or a prestige hull
//!   (`hull.tandem`); or a hullwright service on a part: `:buy:remove:ID`,
//!   `:buy:refit:ID`, `:buy:make:ID`, `:buy:makefit:ID`, `:buy:order:ID`;
//! - `:ask`: ask the dockmaster what changed;
//! - `:reply:N`: pick the Nth open answer at the dock (1-based, as the number keys and
//!   the bubbles' buttons number them), or `:reply:CHOICE` by its ID
//!   (`:reply:choice.side_coop`); then the ticks run so the scripts answer;
//! - `:talk`: click the talk bubble (or press Enter): its next line, or in the open log
//!   the replay's next line; `:talk*N` clicks N times;
//! - `:skip`: the bubble's Skip: what's waiting at this dock goes to the log;
//! - `:log`: open or close the ship log (L, or the log icon);
//! - `:replay:N`: in the open log, replay the Nth visit, newest first (1-based);
//! - `:click:X:Y`, `:qclick:X:Y`: click the play plane at world point X,Y (resolved to a
//!   station, rock or point as the window does), or shift-click to queue it;
//! - `:mapclick:REGION`: click a region on the map (map travel);
//! - `:room:PART`: at the dock, click a part's module (or its key icon) to open its
//!   room, by part ID (`part.wharf.wharf`) or room kind (`:room:market`,
//!   `:room:dockmaster`); `:back`: back to the whole station (Esc, right-click);
//! - `:hover:PART`: show a part's module and key icon hovered, as the mouse would
//!   (`:hover:` with nothing clears it).
//!
//! At the hullwright's cradle (the wharf's room open), the mouse's verbs:
//!
//! - `:tab:ready`, `:tab:make`, `:tab:friends`: click a tray tab;
//! - `:drag:ID:SOCKET`: press on a part (in the tray, the locker, or fitted on its
//!   socket) and drag it over a socket (`socket.drill`) or the locker (`locker`),
//!   still held, so a shot shows the rings, the ghost and the preview; `:drop`: let
//!   go there (the ticks run so the request settles);
//! - `:pick:ID`: click a part (it's picked up; a fitted one is chosen, showing its
//!   Remove); `:socket:ID`: click a socket (drops what's picked up, or chooses what's
//!   in it); `:locker`: click the locker (takes a picked-up fitted part off);
//! - `:remove`: click Remove for the chosen part; `:make:ID`: click a Make part's
//!   Make button (the confirm opens); `:confirm`: click the confirm's Make button;
//!   `:cancel`: put down what's in hand or close the confirm (Esc).
//!
//! The fitting animation runs on the ticks a script runs (`wait*45` plays it out).
//!
//! Example: `w,wait*3,w*75,wait*30,space*150` flies off the dock, flies ahead (east
//! from a new game's Wharf) for two and a half seconds, coasts to a stop, then mines
//! for five.

use galaxy_contract::prelude::*;

use crate::input::{Key, Keys};
use crate::session::Session;
use crate::ui::{self, Amount, UiState};

#[derive(Clone, Debug, PartialEq)]
enum Step {
    Hold(Vec<Key>, u32),
    Pause,
    Map,
    Select(i32),
    Trade(Amount),
    Buy(String),
    Ask,
    Reply(String),
    Talk(u32),
    Skip,
    Log,
    Replay(usize),
    Click { at: (f32, f32), queue: bool },
    MapClick(String),
    Room(String),
    Back,
    Hover(String),
    Wharf(WharfStep),
}

/// A mouse step at the hullwright's cradle.
#[derive(Clone, Debug, PartialEq)]
enum WharfStep {
    Tab(String),
    Drag(String, String),
    Drop,
    Pick(String),
    Socket(String),
    Locker,
    Remove,
    Make(String),
    Confirm,
    Cancel,
}

fn point(raw: &str, rest: &str) -> Result<(f32, f32), String> {
    let (x, y) = rest
        .split_once(':')
        .ok_or_else(|| format!("`{raw}` needs X:Y"))?;
    let n = |s: &str| {
        s.trim()
            .parse::<f32>()
            .map_err(|_| format!("bad number in `{raw}`"))
    };
    Ok((n(x)?, n(y)?))
}

fn key(name: &str) -> Result<Key, String> {
    Ok(match name {
        "w" => Key::W,
        "a" => Key::A,
        "s" => Key::S,
        "d" => Key::D,
        "space" => Key::Space,
        "shift" => Key::Shift,
        other => return Err(format!("unknown key `{other}` in the script")),
    })
}

fn parse(script: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for raw in script.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (token, n) = match raw.split_once('*') {
            Some((t, n)) => (
                t.trim(),
                n.trim()
                    .parse::<u32>()
                    .map_err(|_| format!("bad tick count in `{raw}`"))?,
            ),
            None => (raw, 1),
        };
        steps.push(match token {
            ":pause" => Step::Pause,
            ":map" => Step::Map,
            ":next" => Step::Select(1),
            ":prev" => Step::Select(-1),
            ":trade" => Step::Trade(Amount::One),
            ":tradeall" => Step::Trade(Amount::Most),
            ":ask" => Step::Ask,
            ":talk" => Step::Talk(n),
            ":skip" => Step::Skip,
            ":log" => Step::Log,
            t if t.starts_with(":replay:") => Step::Replay(
                t[8..]
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n >= 1)
                    .ok_or_else(|| format!("`{raw}` needs a visit number from 1"))?,
            ),
            t if t.starts_with(":reply:") => Step::Reply(t[7..].to_owned()),
            t if t.starts_with(":buy:") => Step::Buy(t[5..].to_owned()),
            t if t.starts_with(":click:") => Step::Click {
                at: point(raw, &t[7..])?,
                queue: false,
            },
            t if t.starts_with(":qclick:") => Step::Click {
                at: point(raw, &t[8..])?,
                queue: true,
            },
            t if t.starts_with(":mapclick:") => Step::MapClick(t[10..].to_owned()),
            t if t.starts_with(":room:") => Step::Room(t[6..].to_owned()),
            ":back" => Step::Back,
            t if t.starts_with(":hover:") => Step::Hover(t[7..].to_owned()),
            t if t.starts_with(":tab:") => Step::Wharf(WharfStep::Tab(t[5..].to_owned())),
            t if t.starts_with(":drag:") => {
                let (part, to) = t[6..]
                    .rsplit_once(':')
                    .ok_or_else(|| format!("`{raw}` needs PART:SOCKET"))?;
                Step::Wharf(WharfStep::Drag(part.to_owned(), to.to_owned()))
            }
            ":drop" => Step::Wharf(WharfStep::Drop),
            t if t.starts_with(":pick:") => Step::Wharf(WharfStep::Pick(t[6..].to_owned())),
            t if t.starts_with(":socket:") => Step::Wharf(WharfStep::Socket(t[8..].to_owned())),
            ":locker" => Step::Wharf(WharfStep::Locker),
            ":remove" => Step::Wharf(WharfStep::Remove),
            t if t.starts_with(":make:") => Step::Wharf(WharfStep::Make(t[6..].to_owned())),
            ":confirm" => Step::Wharf(WharfStep::Confirm),
            ":cancel" => Step::Wharf(WharfStep::Cancel),
            "wait" => Step::Hold(Vec::new(), n),
            keys => Step::Hold(
                keys.split('+')
                    .map(str::trim)
                    .map(key)
                    .collect::<Result<_, _>>()?,
                n,
            ),
        });
    }
    Ok(steps)
}

/// Runs `script` on `session`. Returns the ticks run.
pub fn run(session: &mut Session, ui_state: &mut UiState, script: &str) -> Result<u64, String> {
    let mut ticks = 0;
    let mut keys = Keys::default();
    for step in parse(script)? {
        let before = ticks;
        let wharf_step = matches!(step, Step::Wharf(_));
        match step {
            Step::Hold(held, n) => {
                for k in &held {
                    keys.press(*k);
                }
                for _ in 0..n {
                    tick(session, &mut keys);
                    ticks += 1;
                }
                // Released, as the window would see it: a press no tick took yet stays
                // latched for the next step.
                for k in &held {
                    keys.release(*k);
                }
            }
            Step::Pause => {
                session.toggle_pause();
                keys.clear();
            }
            Step::Map => ui_state.map_open = !ui_state.map_open,
            Step::Select(d) => {
                let n = docked_lines(session).len().max(1) as i32;
                ui_state.selected = (ui_state.selected as i32 + d).rem_euclid(n) as usize;
            }
            Step::Trade(amount) => {
                let Some(station) = session.flagship().and_then(|f| f.docked_at.clone()) else {
                    continue;
                };
                let lines = docked_lines(session);
                if let Some(line) = lines.get(ui_state.selected)
                    && let Some(req) = ui::trade_request(session.world(), &station, line, amount)
                {
                    session.request_trade(req);
                }
                // Let it settle.
                for _ in 0..3 {
                    tick(session, &mut keys);
                    ticks += 1;
                }
            }
            Step::Buy(id) => {
                session.request_service(service(&id)?);
                for _ in 0..3 {
                    tick(session, &mut keys);
                    ticks += 1;
                }
            }
            Step::Ask => {
                session.ask_what_changed();
                // The reply, the pack's answer, and the digest lines it leaves.
                for _ in 0..4 {
                    tick(session, &mut keys);
                    ticks += 1;
                }
            }
            Step::Reply(which) => {
                let Some(station) = session.flagship().and_then(|f| f.docked_at.clone()) else {
                    return Err(format!("`:reply:{which}` needs the ship docked"));
                };
                let open =
                    crate::hooks::open_choices(session.world(), session.sim().history(), &station);
                let index = match which.parse::<usize>() {
                    Ok(n) if n >= 1 => Some(n - 1),
                    Ok(_) => None,
                    Err(_) => open.iter().position(|c| c.choice.as_str() == which),
                };
                let Some(i) = index.filter(|i| *i < open.len()) else {
                    let offered: Vec<&str> = open.iter().map(|c| c.choice.as_str()).collect();
                    return Err(format!(
                        "`:reply:{which}`: no such answer open here (open: {offered:?})"
                    ));
                };
                session.reply_to_open(i);
                for _ in 0..4 {
                    tick(session, &mut keys);
                    ticks += 1;
                }
            }
            Step::Talk(n) => {
                for _ in 0..n {
                    ui_state.enter(session);
                }
            }
            Step::Skip => session.skip_talk(),
            Step::Log => ui_state.toggle_log(&mut session.talk),
            Step::Replay(n) => {
                if !ui_state.log_open {
                    return Err(format!("`:replay:{n}` needs the log open (`:log`)"));
                }
                let (world, history) = (session.world(), session.sim().history());
                let visits = ui::log::visits(world, history);
                let from = ui::log::entries(world, &visits)
                    .into_iter()
                    .filter_map(|e| match e {
                        ui::log::Entry::Visit(i) if !visits[i].bubbles().is_empty() => {
                            Some(visits[i].from)
                        }
                        _ => None,
                    })
                    .nth(n - 1)
                    .ok_or_else(|| format!("`:replay:{n}`: the log has fewer visits"))?;
                ui_state.replay = Some(ui::log::Replay { from, line: 0 });
            }
            Step::Click { at, queue } => {
                let world = session.world();
                let Some(region) = session.flagship().map(|f| f.region.clone()) else {
                    continue;
                };
                let target = crate::input::click_target(world, &region, Vec2::new(at.0, at.1));
                click(session, &mut keys, Click { target, queue });
                ticks += 1;
            }
            Step::Room(which) => {
                let id = docked_part(session, &which)?;
                if ui_state.room.as_ref() != Some(&id) {
                    ui_state.selected = 0;
                    ui_state.services_first = 0;
                    ui_state.wharf.reset();
                }
                ui_state.room = Some(id);
            }
            Step::Back => {
                ui_state.room = None;
                ui_state.wharf.reset();
            }
            Step::Wharf(w) => {
                if let Some(req) = wharf(session, ui_state, &w)? {
                    ui::rooms::fit::send(session, &mut ui_state.wharf, req);
                    for _ in 0..3 {
                        tick(session, &mut keys);
                        ticks += 1;
                    }
                }
            }
            Step::Hover(which) => {
                ui_state.hover = if which.is_empty() {
                    None
                } else {
                    Some(docked_part(session, &which)?)
                };
            }
            Step::MapClick(region) => {
                let region: RegionId = region.parse().map_err(|e| format!("{e}"))?;
                let queue = keys.held(Key::Shift);
                click(
                    session,
                    &mut keys,
                    Click {
                        target: ClickTarget::Region { region },
                        queue,
                    },
                );
                ticks += 1;
            }
        }
        if session.flagship().is_some_and(|f| f.docked_at.is_none()) {
            ui_state.room = None;
            ui_state.wharf.reset();
        }
        ui::rooms::fit::settle(session, &mut ui_state.wharf);
        let ran = ticks - before;
        // A wharf step's own settling ticks happen as the animation starts.
        if ran > 0 && !wharf_step {
            ui_state
                .wharf
                .advance(ran as f32 / galaxy_contract::TICKS_PER_SECOND as f32);
        }
    }
    Ok(ticks)
}

/// A mouse step at the cradle: what the window's press, move, release or click would
/// do there. Returns the request it sends.
fn wharf(
    session: &Session,
    ui_state: &mut UiState,
    step: &WharfStep,
) -> Result<Option<ServiceRequest>, String> {
    use ui::rooms::fit::{Over, Source, WharfAction};
    if crate::frame::cradle_room(session, ui_state).is_none() {
        return Err(format!(
            "`{step:?}` needs a wharf's room open at the dock (`:room:wharf`)"
        ));
    }
    let station = session
        .flagship()
        .and_then(|f| f.docked_at.clone())
        .expect("docked");
    let (world, history) = (session.world(), session.sim().history());
    let w = &mut ui_state.wharf;
    let upgrade = |id: &str| id.parse::<UpgradeId>().map_err(|e| format!("`{id}`: {e}"));
    let socket = |id: &str| id.parse::<SocketId>().map_err(|e| format!("`{id}`: {e}"));
    // A part as the mouse finds it: fitted on its socket, in the locker, or in the tray.
    let part_action = |part: UpgradeId| {
        if world.player.fittings.contains(&part) {
            ui::rooms::fit::socket_of(world, &part).map_or(
                WharfAction::Part {
                    part: part.clone(),
                    from: Source::Socket,
                },
                WharfAction::Socket,
            )
        } else {
            let in_locker = galaxy_contract::fit::flagship(world)
                .is_some_and(|s| galaxy_contract::fit::locker(world, &s.id).any(|p| p == &part));
            WharfAction::Part {
                part,
                from: if in_locker {
                    Source::Locker
                } else {
                    Source::Tray
                },
            }
        }
    };
    Ok(match step {
        WharfStep::Tab(t) => {
            let tab = ui::rooms::fit::Tab::parse(t)
                .ok_or_else(|| format!("`{t}`: the tabs are ready, make and friends"))?;
            w.click(world, history, &station, &WharfAction::Tab(tab))
        }
        WharfStep::Drag(part, to) => {
            let a = part_action(upgrade(part)?);
            if !w.press(world, &a) {
                return Err(format!("`{part}` can't be picked up now"));
            }
            let over = if to == "locker" {
                Over::Locker
            } else {
                Over::Socket(socket(to)?)
            };
            w.hover(Some(over));
            None
        }
        WharfStep::Drop => w.release(world, history, true),
        WharfStep::Pick(part) => {
            let a = part_action(upgrade(part)?);
            if w.press(world, &a) {
                w.release(world, history, false)
            } else {
                w.click(world, history, &station, &a)
            }
        }
        WharfStep::Socket(s) => w.click(world, history, &station, &WharfAction::Socket(socket(s)?)),
        WharfStep::Locker => w.click(world, history, &station, &WharfAction::Locker),
        WharfStep::Remove => {
            let part = w
                .chosen
                .clone()
                .ok_or("`:remove` needs a fitted part chosen (`:pick:ID` on it)")?;
            w.click(world, history, &station, &WharfAction::Remove(part))
        }
        WharfStep::Make(part) => w.click(
            world,
            history,
            &station,
            &WharfAction::Make {
                part: upgrade(part)?,
                fit: false,
            },
        ),
        WharfStep::Confirm => {
            if w.make.is_none() {
                return Err("`:confirm` needs the Make confirm open".into());
            }
            w.click(world, history, &station, &WharfAction::ConfirmMake)
        }
        WharfStep::Cancel => {
            w.back();
            None
        }
    })
}

/// The service request a dock-screen button would make for `id`. The hullwright's
/// other services take a word first: `remove:ID`, `refit:ID`, `make:ID`,
/// `makefit:ID` (make and fit) and `order:ID`.
fn service(id: &str) -> Result<ServiceRequest, String> {
    let bad = |e: galaxy_contract::ids::IdError| format!("`{id}`: {e}");
    if let Some((verb, part)) = id.split_once(':') {
        let upgrade: UpgradeId = part.parse().map_err(bad)?;
        return Ok(match verb {
            "remove" => ServiceRequest::Remove { upgrade },
            "refit" => ServiceRequest::Refit { upgrade },
            "make" => ServiceRequest::Make {
                upgrade,
                fit: false,
            },
            "makefit" => ServiceRequest::Make { upgrade, fit: true },
            "order" => ServiceRequest::Order { upgrade },
            _ => return Err(format!("`{verb}`: not a hullwright service")),
        });
    }
    Ok(if id.starts_with("upgrade.") {
        ServiceRequest::Fit {
            upgrade: id.parse().map_err(bad)?,
        }
    } else if id.starts_with("hull.") {
        ServiceRequest::Prestige {
            to: id.parse().map_err(bad)?,
        }
    } else {
        ServiceRequest::Build {
            craft: id.parse().map_err(bad)?,
        }
    })
}

/// One tick with the keys, as the window runs it: none while paused.
fn tick(session: &mut Session, keys: &mut Keys) {
    if session.is_paused() {
        return;
    }
    let docked = session.flagship().is_some_and(|f| f.docked_at.is_some());
    session.tick(keys.tick_input(docked, None));
}

/// One tick carrying a click, as the window sends it with the next tick.
fn click(session: &mut Session, keys: &mut Keys, click: Click) {
    if session.is_paused() {
        return;
    }
    let docked = session.flagship().is_some_and(|f| f.docked_at.is_some());
    session.tick(keys.tick_input(docked, Some(click)));
}

/// The room at the dock (a part, or a stand-in) named by ID or by room kind (`market`,
/// `wharf`, `dockmaster`).
fn docked_part(session: &Session, which: &str) -> Result<StationPartId, String> {
    let Some(station) = session.flagship().and_then(|f| f.docked_at.clone()) else {
        return Err(format!("`{which}`: rooms need the ship docked"));
    };
    let world = session.world();
    let parts = ui::station::rooms_here(world, &station);
    parts
        .iter()
        .find(|p| {
            p.id.as_str() == which
                || serde_json::to_value(p.room)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    == Some(which.to_owned())
        })
        .map(|p| p.id.clone())
        .ok_or_else(|| {
            let ids: Vec<&str> = parts.iter().map(|p| p.id.as_str()).collect();
            format!("`{which}`: no such part at {station} (it has {ids:?})")
        })
}

fn docked_lines(session: &Session) -> Vec<ui::DockLine> {
    session
        .flagship()
        .and_then(|f| f.docked_at.clone())
        .map(|s| ui::dock_lines(session.world(), &s))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_parse() {
        assert_eq!(
            parse("w+d*30, space*2,:pause,wait*5,:tradeall").unwrap(),
            vec![
                Step::Hold(vec![Key::W, Key::D], 30),
                Step::Hold(vec![Key::Space], 2),
                Step::Pause,
                Step::Hold(vec![], 5),
                Step::Trade(Amount::Most),
            ]
        );
        assert_eq!(
            parse(":reply:2,:reply:choice.side_coop").unwrap(),
            vec![
                Step::Reply("2".into()),
                Step::Reply("choice.side_coop".into())
            ]
        );
        assert_eq!(
            parse(":talk*3,:skip,:log,:replay:2").unwrap(),
            vec![Step::Talk(3), Step::Skip, Step::Log, Step::Replay(2)]
        );
        assert_eq!(
            parse(":room:market,:hover:part.wharf.wharf,:back,:hover:").unwrap(),
            vec![
                Step::Room("market".into()),
                Step::Hover("part.wharf.wharf".into()),
                Step::Back,
                Step::Hover(String::new()),
            ]
        );
        assert_eq!(
            parse(":tab:make,:drag:upgrade.mining_lens:socket.drill,:drop,:pick:upgrade.hold_pods")
                .unwrap(),
            vec![
                Step::Wharf(WharfStep::Tab("make".into())),
                Step::Wharf(WharfStep::Drag(
                    "upgrade.mining_lens".into(),
                    "socket.drill".into()
                )),
                Step::Wharf(WharfStep::Drop),
                Step::Wharf(WharfStep::Pick("upgrade.hold_pods".into())),
            ]
        );
        assert!(parse(":drag:upgrade.mining_lens").is_err());
        assert!(parse(":replay:0").is_err());
        assert!(parse("x*3").is_err());
        assert!(parse("w*lots").is_err());
    }
}

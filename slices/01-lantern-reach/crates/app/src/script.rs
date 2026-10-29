//! Scripted input for `galaxy shot --keys`: fly a save headless with the same input
//! mapping the window uses, so agents (and tests) can look at the game mid-play.
//!
//! A script is comma-separated steps, each `STEP` or `STEP*N` (N ticks, default 1):
//!
//! - keys joined by `+`, held for N ticks then released: `w`, `a`, `s`, `d`, `space`,
//!   `shift`, `w+d`, `space*90`;
//! - `wait*N`: hands off for N ticks;
//! - `:pause`, `:map`: toggle the PAUSED card or the map;
//! - `:next`, `:prev`: move the dock screen's selection;
//! - `:trade`, `:tradeall`: trade one, or all, of the selected dock line (then the
//!   ticks run so the trade settles).
//!
//! Example: `d*120,space*150` flies right for four seconds, then mines for five.

use crate::input::{Key, Keys, player_input};
use crate::session::Session;
use crate::ui::{self, Amount, UiState};

#[derive(Clone, Debug, PartialEq)]
enum Step {
    Hold(Vec<Key>, u32),
    Pause,
    Map,
    Select(i32),
    Trade(Amount),
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
        match step {
            Step::Hold(held, n) => {
                for k in &held {
                    keys.press(*k);
                }
                for _ in 0..n {
                    let docked = session.flagship().is_some_and(|f| f.docked_at.is_some());
                    session.tick(player_input(&keys, docked, None));
                    ticks += 1;
                }
                keys.clear();
            }
            Step::Pause => {
                session.toggle_pause();
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
                    session.tick(player_input(&keys, true, None));
                    ticks += 1;
                }
            }
        }
    }
    Ok(ticks)
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
        assert!(parse("x*3").is_err());
        assert!(parse("w*lots").is_err());
    }
}

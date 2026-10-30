//! Evaluating [`Requirement`]s and [`Measure`]s against the world and history.
//!
//! Everything here reads activity, world state and the order of history facts. Nothing
//! reads time: "since the beat started" means "facts after the history head when it
//! started", so a requirement can never be a countdown.

use galaxy_contract::prelude::*;
use galaxy_contract::progress::{Measure, Num, Since};
use galaxy_core::History;

/// What a requirement is evaluated against.
#[derive(Clone, Copy)]
pub struct Eval<'a> {
    pub world: &'a WorldState,
    pub history: &'a History,
    /// The beat whose requirement this is, for `since = "start"` / `"step"`.
    pub beat: Option<&'a BeatState>,
}

/// A requirement that can't be evaluated as written.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum EvalError {
    #[error("tunable `{0}` doesn't exist or isn't a number")]
    Tunable(TunableId),
    #[error("`test` requirement {0} has no predicate here")]
    NoPredicate(u32),
    #[error("{0}")]
    Predicate(String),
}

impl<'a> Eval<'a> {
    pub fn new(world: &'a WorldState, history: &'a History) -> Self {
        Self {
            world,
            history,
            beat: None,
        }
    }

    pub fn for_beat(mut self, beat: Option<&'a BeatState>) -> Self {
        self.beat = beat;
        self
    }

    /// A number, resolving tunables.
    pub fn num(&self, n: &Num) -> Result<f64, EvalError> {
        match n {
            Num::Value(v) => Ok(*v),
            Num::Tunable { tunable } => self
                .world
                .progress
                .tunables
                .get(tunable)
                .and_then(|t| t.value.as_number())
                .ok_or_else(|| EvalError::Tunable(tunable.clone())),
        }
    }

    fn after(&self, since: Since) -> Option<FactId> {
        let b = self.beat?;
        match since {
            Since::Ever => None,
            Since::Start => b.since,
            Since::Step => b.step_since.or(b.since),
        }
    }

    fn windowed(&self, q: &HistoryQuery, since: Since) -> HistoryQuery {
        let mut q = q.clone();
        if let Some(a) = self.after(since) {
            q.after = Some(q.after.map_or(a, |x| x.max(a)));
        }
        q
    }

    /// Does `req` hold? `test` requirements call `tests` with their index.
    pub fn holds(
        &self,
        req: &Requirement,
        tests: &mut dyn FnMut(u32) -> Result<bool, EvalError>,
    ) -> Result<bool, EvalError> {
        use Requirement as R;
        let w = self.world;
        let ship = player_ship(w);
        let beat = |id: &BeatId| w.progress.beats.get(id);
        Ok(match req {
            R::Always => true,
            R::All(list) => {
                for r in list {
                    if !self.holds(r, tests)? {
                        return Ok(false);
                    }
                }
                true
            }
            R::Any(list) => {
                for r in list {
                    if self.holds(r, tests)? {
                        return Ok(true);
                    }
                }
                false
            }
            R::Not(r) => !self.holds(r, tests)?,
            R::BeatDone(id) => beat(id).is_some_and(|b| b.status == BeatStatus::Completed),
            R::BeatStarted(id) => beat(id).is_some(),
            R::BeatActive(id) => beat(id).is_some_and(|b| b.status == BeatStatus::Active),
            R::BeatStep { beat: id, at_least } => {
                beat(id).is_some_and(|b| b.status == BeatStatus::Completed || b.step >= *at_least)
            }
            R::Counter {
                kind,
                at_least,
                station,
                region,
            } => {
                let scope = scope(station, region);
                w.counters.get(*kind, &scope) as f64 >= self.num(at_least)?
            }
            R::Facts {
                query,
                at_least,
                since,
            } => {
                let need = self.num(at_least)?;
                let q = self.windowed(query, *since);
                if need <= 1.0 {
                    need <= 0.0 || self.history.last(&q).is_some()
                } else {
                    self.history.count(&q) as f64 >= need
                }
            }
            R::Sum {
                query,
                at_least,
                since,
            } => sum(self.history, &self.windowed(query, *since)) as f64 >= self.num(at_least)?,
            R::Flag(f) => w.player.flags.contains(f),
            R::Trait(t) => w.player.traits.contains(t),
            R::Fitted(u) => w.player.fittings.contains(u),
            R::Hull(h) => ship.is_some_and(|s| &s.hull == h),
            R::Owns(id) => w.ships.get(id).is_some_and(|s| s.owner == Owner::Player),
            R::Credits { at_least } => w.player.credits as f64 >= self.num(at_least)?,
            R::Cargo { ware, at_least } => {
                f64::from(ship.map_or(0, |s| s.hold.count(ware))) >= self.num(at_least)?
            }
            R::HoldFree { at_least } => hold_free(w, ship) as f64 >= self.num(at_least)?,
            R::DockedAt(st) => ship.is_some_and(|s| s.docked_at.as_ref() == Some(st)),
            R::Docked => ship.is_some_and(|s| s.docked_at.is_some()),
            R::InRegion(r) => ship.is_some_and(|s| &s.region == r),
            R::Near {
                region,
                x,
                y,
                within,
            } => match ship.filter(|s| &s.region == region) {
                Some(s) => {
                    let at = Vec2::new(self.num(x)? as f32, self.num(y)? as f32);
                    f64::from(s.position.distance(at)) <= self.num(within)?
                }
                None => false,
            },
            R::Regard { faction, at_least } => {
                match (
                    crate::standing::tier_index(w, faction),
                    w.progress
                        .rules
                        .regard
                        .iter()
                        .position(|t| &t.id == at_least),
                ) {
                    (Some(have), Some(need)) => have >= need,
                    _ => false,
                }
            }
            R::TunableIs { tunable, value } => w
                .progress
                .tunables
                .get(tunable)
                .is_some_and(|t| &t.value == value),
            R::Test(i) => tests(*i)?,
        })
    }

    /// `holds` for requirements that can't contain predicates (data files).
    pub fn holds_data(&self, req: &Requirement) -> bool {
        self.holds(req, &mut |i| Err(EvalError::NoPredicate(i)))
            .unwrap_or(false)
    }
}

/// The counter scope a requirement names (the player's unless a station or region).
pub fn scope(station: &Option<StationId>, region: &Option<RegionId>) -> CounterScope {
    match (station, region) {
        (Some(station), _) => CounterScope::Station {
            station: station.clone(),
        },
        (None, Some(region)) => CounterScope::Region {
            region: region.clone(),
        },
        (None, None) => CounterScope::Player,
    }
}

/// The ship the player flies.
pub fn player_ship(world: &WorldState) -> Option<&Ship> {
    world
        .player
        .ship
        .as_ref()
        .and_then(|id| world.ships.get(id))
}

/// Free hold units on a ship, counting hold modifiers.
pub fn hold_free(world: &WorldState, ship: Option<&Ship>) -> u64 {
    let Some(ship) = ship else { return 0 };
    let cap = world
        .hulls
        .get(&ship.hull)
        .map_or(0, |h| u64::from(h.hold_capacity))
        + u64::from(crate::modifiers::for_ship(world, &ship.id).hold);
    cap.saturating_sub(ship.hold.total())
}

/// How many times a deed was done. `after` narrows history measures to facts after
/// that one (a hull's rank window); counters can't be narrowed.
pub fn measure(
    world: &WorldState,
    history: &History,
    m: &Measure,
    after: Option<FactId>,
    until: Option<FactId>,
) -> u64 {
    let bounded = |q: &HistoryQuery| {
        let mut q = q.clone();
        if let Some(a) = after {
            q.after = Some(q.after.map_or(a, |x| x.max(a)));
        }
        q
    };
    let cut = |f: &Fact| until.is_none_or(|u| f.id <= u);
    match m {
        Measure::Counter {
            kind,
            station,
            region,
        } => world.counters.get(*kind, &scope(station, region)),
        Measure::Facts(q) => {
            let q = bounded(q);
            match until {
                None => history.count(&q) as u64,
                Some(_) => history.find(&q).into_iter().filter(|f| cut(f)).count() as u64,
            }
        }
        Measure::Sum(q) => {
            let q = bounded(q);
            let total: i64 = match until {
                None => sum(history, &q),
                Some(_) => history
                    .find(&q)
                    .into_iter()
                    .filter(|f| cut(f))
                    .filter_map(|f| f.amount)
                    .fold(0i64, i64::saturating_add),
            };
            u64::try_from(total).unwrap_or(0)
        }
    }
}

/// Matching facts' amounts added up, saturating instead of failing when they don't fit
/// in an `i64` (scripts write amounts too).
fn sum(history: &History, q: &HistoryQuery) -> i64 {
    history.sum(q).unwrap_or_else(|| {
        let all = HistoryQuery {
            limit: None,
            ..q.clone()
        };
        history
            .find(&all)
            .into_iter()
            .filter_map(|f| f.amount)
            .fold(0i64, i64::saturating_add)
    })
}

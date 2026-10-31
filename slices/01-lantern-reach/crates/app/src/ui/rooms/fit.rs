//! Drag to fit at the hullwright's cradle (#47, 3.2): what the player is holding, where
//! it is over, and what letting go there asks for. Pure state over the world, so the
//! whole pick → hover → drop → request path is tested headless
//! (`crates/app/tests/wharf_fit.rs`); the window feeds it the mouse and `galaxy shot`
//! feeds it key-script verbs.
//!
//! - **Drag**: the mouse goes down on a part (a tray row, a locker chip, a fitted part
//!   on its socket), moves, and comes up over a socket or the locker.
//! - **Click**: the same part clicked and let go where it was is picked up; the next
//!   click on a socket (or the locker) is the drop. A fitted part clicked on its socket
//!   is chosen instead, and its Remove choice shows in the room.
//! - **The drop** asks the hullwright's preview first ([`galaxy_progress::hullwright`]).
//!   A drop the hullwright would refuse sends nothing and leaves the keeper's reason, in
//!   their own words. A Make part opens the Make confirm (what it uses, before anything
//!   is spent). Anything else is one [`ServiceRequest`], and the fitting animation
//!   ([`Fitting`]) starts: view only, about a second and a half, a click skips it.

use galaxy_contract::fit;
use galaxy_contract::prelude::*;
use galaxy_core::History;
use galaxy_progress::hullwright::{self, Preview, Stats};

/// The tray's tabs: where parts come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tab {
    /// Bought for credits.
    #[default]
    Ready,
    /// Made from materials and a blueprint.
    Make,
    /// Sent by a friend at another station.
    Friends,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Ready, Tab::Make, Tab::Friends];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Ready => "Ready",
            Tab::Make => "Make",
            Tab::Friends => "Friends",
        }
    }

    /// From a key-script word.
    pub fn parse(word: &str) -> Option<Tab> {
        Tab::ALL
            .into_iter()
            .find(|t| t.label().eq_ignore_ascii_case(word))
    }
}

/// Where a held part was picked up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A tray row (Ready or Make).
    Tray,
    /// The hull's locker.
    Locker,
    /// Its socket on the ship: it's fitted.
    Socket,
}

/// A part in the hand.
#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub part: UpgradeId,
    pub from: Source,
    /// The mouse button is down (a drag); `false`: picked up by a click.
    pub dragging: bool,
}

/// What the held part is over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Over {
    Socket(SocketId),
    Locker,
}

/// The fitting animation: the crane swings over, lowers the part, three bolt flashes,
/// the hull dips and settles. View only; the world has the part already.
#[derive(Clone, Debug, PartialEq)]
pub struct Fitting {
    pub part: UpgradeId,
    pub socket: SocketId,
    /// Seconds in.
    pub t: f32,
}

/// How long the fitting animation plays.
pub const FIT_SECONDS: f32 = 1.5;

/// What a button at the hullwright does.
#[derive(Clone, Debug, PartialEq)]
pub enum WharfAction {
    /// Show a tray tab.
    Tab(Tab),
    /// A part to pick up: a tray row, a locker chip.
    Part { part: UpgradeId, from: Source },
    /// A socket in the cradle (its ring, or its label beside the ship).
    Socket(SocketId),
    /// The locker area: a drop here takes a fitted part off.
    Locker,
    /// Take the chosen fitted part off, into the locker.
    Remove(UpgradeId),
    /// Open the Make confirm for a part; `fit`: make it and fit it.
    Make { part: UpgradeId, fit: bool },
    /// Make the part the confirm shows.
    ConfirmMake,
    /// Put down what's held, close the confirm or the chosen part's choice.
    Cancel,
    /// A control module in the rack: it stays.
    Module(UpgradeId),
    /// Show the tray from this row.
    From(usize),
    /// Show this page of the locker's chips.
    LockerPage(usize),
}

/// What the keeper says in the room: their reason for a refusal, or a word about what
/// is held. Always in the first person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    pub text: String,
    /// A refusal: drawn with the vermilion bar and its word.
    pub refusal: bool,
}

/// The hullwright room's state, beside [`super::super::UiState`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WharfUi {
    pub tab: Tab,
    pub held: Option<Held>,
    pub over: Option<Over>,
    /// A fitted part clicked on its socket: its Remove choice shows.
    pub chosen: Option<UpgradeId>,
    /// The Make confirm, open for this part; `true`: make and fit.
    pub make: Option<(UpgradeId, bool)>,
    pub fitting: Option<Fitting>,
    pub said: Option<Said>,
    /// The latest request sent from here, for the keeper's line once it settles.
    pub sent: Option<ServiceRequest>,
    /// The first tray row shown.
    pub first: usize,
    /// The locker's page of chips shown, when they don't all fit in one row.
    pub locker_page: usize,
}

/// A held part's preview against where it is over: the hullwright's for a fit, or the
/// stats and budgets without it for a fitted part going to the locker.
#[derive(Clone, Debug, PartialEq)]
pub struct HeldPreview {
    pub part: UpgradeId,
    pub name: String,
    /// The socket it goes in (a fit) or comes out of (a removal).
    pub socket: SocketId,
    pub removing: bool,
    pub before: Stats,
    pub after: Stats,
    pub budget_before: fit::Budget,
    pub budget_after: fit::Budget,
    /// The part a fit puts in the locker.
    pub swaps_out: Option<UpgradeId>,
    pub request: ServiceRequest,
    /// Why the hullwright would refuse it, in their words.
    pub refusal: Option<String>,
}

impl WharfUi {
    /// Starts over: a room opened, closed or left.
    pub fn reset(&mut self) {
        *self = Self {
            tab: self.tab,
            ..Self::default()
        };
    }

    /// Whether anything modal is up (a confirm) or held.
    pub fn busy(&self) -> bool {
        self.held.is_some() || self.make.is_some() || self.chosen.is_some()
    }

    /// The animation's clock: `dt` real (or scripted) seconds on.
    pub fn advance(&mut self, dt: f32) {
        if let Some(f) = &mut self.fitting {
            f.t += dt.max(0.0);
            if f.t >= FIT_SECONDS {
                self.fitting = None;
            }
        }
    }

    /// A click during the animation ends it.
    pub fn skip(&mut self) -> bool {
        self.fitting.take().is_some()
    }

    /// Esc or a right-click: puts down what's held, or closes a confirm or a choice.
    /// `false` when there was nothing to put down.
    pub fn back(&mut self) -> bool {
        let busy = self.busy();
        self.held = None;
        self.over = None;
        self.make = None;
        self.chosen = None;
        busy
    }

    /// The mouse went down on `action`: a part there is picked up and dragged. A
    /// socket with a part in it, while nothing is held, picks that part up. `true` when
    /// a drag began (the press is the room's, nothing else gets it).
    pub fn press(&mut self, world: &WorldState, action: &WharfAction) -> bool {
        if self.fitting.is_some() || self.make.is_some() {
            return false;
        }
        let picked = match action {
            WharfAction::Part { part, from } => Some((part.clone(), *from)),
            WharfAction::Socket(s) if self.held.is_none() => {
                occupant(world, s).map(|p| (p, Source::Socket))
            }
            _ => None,
        };
        let Some((part, from)) = picked else {
            return false;
        };
        self.chosen = None;
        self.said = None;
        self.held = Some(Held {
            part,
            from,
            dragging: true,
        });
        self.over = None;
        true
    }

    /// The held part is over `over` now (the mouse moved, or a script moved it).
    pub fn hover(&mut self, over: Option<Over>) {
        if self.held.is_some() {
            self.over = over;
        }
    }

    /// The mouse came up. `moved`: it travelled since it went down, so this is the end
    /// of a drag; else the press was a click, and the part stays picked up (a fitted
    /// one is chosen instead). Returns the request a drop sends.
    pub fn release(
        &mut self,
        world: &WorldState,
        history: &History,
        moved: bool,
    ) -> Option<ServiceRequest> {
        let held = self.held.as_mut().filter(|h| h.dragging)?;
        if !moved {
            if held.from == Source::Socket {
                self.chosen = Some(held.part.clone());
                self.held = None;
                self.over = None;
            } else {
                held.dragging = false;
            }
            return None;
        }
        let over = self.over.clone();
        match over {
            Some(o) => self.drop_on(world, history, o),
            None => {
                // Let go over nothing: it goes back where it was.
                self.held = None;
                None
            }
        }
    }

    /// A click on one of the room's buttons. Returns a request to send.
    pub fn click(
        &mut self,
        world: &WorldState,
        history: &History,
        station: &StationId,
        action: &WharfAction,
    ) -> Option<ServiceRequest> {
        match action {
            WharfAction::Tab(t) => {
                if self.tab != *t {
                    self.tab = *t;
                    self.first = 0;
                }
                None
            }
            WharfAction::From(n) => {
                self.first = *n;
                None
            }
            WharfAction::LockerPage(n) => {
                self.locker_page = *n;
                None
            }
            WharfAction::Part { part, from } => {
                // Clicked again: put it back. Else pick this one up.
                if self.held.as_ref().is_some_and(|h| &h.part == part) {
                    self.held = None;
                } else {
                    self.press(world, action);
                    if let Some(h) = &mut self.held {
                        h.dragging = false;
                        h.from = *from;
                    }
                }
                None
            }
            WharfAction::Socket(s) => {
                if self.held.is_some() {
                    self.over = Some(Over::Socket(s.clone()));
                    return self.drop_on(world, history, Over::Socket(s.clone()));
                }
                // An empty socket: what goes there. A fitted one: its choice.
                match occupant(world, s) {
                    Some(p) => {
                        self.chosen = Some(p);
                        self.said = None;
                    }
                    None => {
                        self.chosen = None;
                        self.said = Some(Said {
                            text: empty_socket_line(world, station, s),
                            refusal: false,
                        });
                    }
                }
                None
            }
            WharfAction::Locker => {
                if self.held.is_some() {
                    self.over = Some(Over::Locker);
                    return self.drop_on(world, history, Over::Locker);
                }
                None
            }
            WharfAction::Remove(part) => {
                self.chosen = None;
                Some(ServiceRequest::Remove {
                    upgrade: part.clone(),
                })
            }
            WharfAction::Make { part, fit } => {
                self.held = None;
                self.over = None;
                self.chosen = None;
                self.make = Some((part.clone(), *fit));
                None
            }
            WharfAction::ConfirmMake => {
                let (part, fit) = self.make.take()?;
                let req = ServiceRequest::Make {
                    upgrade: part.clone(),
                    fit,
                };
                if fit && let Some(s) = socket_of(world, &part) {
                    self.start_fitting(part, s);
                }
                Some(req)
            }
            WharfAction::Cancel => {
                self.back();
                None
            }
            WharfAction::Module(_) => {
                self.said = Some(Said {
                    text: "I don't take controls off. Once they're yours, they stay with you."
                        .into(),
                    refusal: false,
                });
                None
            }
        }
    }

    /// Lets go of the held part on `over`.
    fn drop_on(
        &mut self,
        world: &WorldState,
        history: &History,
        over: Over,
    ) -> Option<ServiceRequest> {
        let held = self.held.take()?;
        self.over = None;
        let Some(p) = preview(world, history, &held, &over) else {
            // Nothing to do there (a part from the tray let go on the locker).
            return None;
        };
        let target_ok = match &over {
            Over::Socket(s) => *s == p.socket,
            Over::Locker => p.removing,
        };
        if !target_ok {
            let text = match (&over, held.from) {
                (Over::Locker, _) => {
                    "The locker takes parts off the ship. Fit this one first.".to_owned()
                }
                (Over::Socket(_), Source::Socket) => {
                    "That stays where it is. Drag it to the locker to take it off.".to_owned()
                }
                (Over::Socket(_), _) => format!(
                    "That goes on the {}.",
                    socket_name(world, &p.socket).to_lowercase()
                ),
            };
            self.said = Some(Said {
                text,
                refusal: true,
            });
            return None;
        }
        if let Some(r) = p.refusal {
            self.said = Some(Said {
                text: r,
                refusal: true,
            });
            return None;
        }
        self.said = None;
        if let ServiceRequest::Make { upgrade, .. } = &p.request {
            // Making spends materials: the confirm shows them first.
            self.make = Some((upgrade.clone(), true));
            return None;
        }
        if !p.removing {
            self.start_fitting(p.part.clone(), p.socket.clone());
        }
        self.sent = Some(p.request.clone());
        Some(p.request)
    }

    fn start_fitting(&mut self, part: UpgradeId, socket: SocketId) {
        self.fitting = Some(Fitting {
            part,
            socket,
            t: 0.0,
        });
    }

    /// The request went out (from a drop, a button or a key script): remember it for
    /// the keeper's line.
    pub fn sending(&mut self, req: &ServiceRequest) {
        self.sent = Some(req.clone());
        self.said = None;
    }

    /// A request the desk refused at once (or after its tick): no animation for it.
    pub fn refused(&mut self) {
        self.fitting = None;
    }

    /// What the held part would do where it is (or, over nothing, where it goes).
    pub fn held_preview(&self, world: &WorldState, history: &History) -> Option<HeldPreview> {
        let held = self.held.as_ref()?;
        let over = self.over.clone().unwrap_or(match held.from {
            Source::Socket => Over::Locker,
            _ => Over::Socket(socket_of(world, &held.part)?),
        });
        preview(world, history, held, &over)
    }
}

/// The part (not a control module) in socket `s` now.
pub fn occupant(world: &WorldState, s: &SocketId) -> Option<UpgradeId> {
    if fit::is_rack(world, s) {
        return None;
    }
    fit::occupant(world, &world.player.fittings, s).cloned()
}

/// The socket a part goes in.
pub fn socket_of(world: &WorldState, part: &UpgradeId) -> Option<SocketId> {
    world.progress.rules.upgrades.get(part)?.socket.clone()
}

/// A socket's name ("Drill mount").
pub fn socket_name(world: &WorldState, s: &SocketId) -> String {
    world
        .progress
        .rules
        .fit
        .sockets
        .get(s)
        .map_or_else(|| s.to_string(), |d| d.name.clone())
}

pub fn part_name(world: &WorldState, part: &UpgradeId) -> String {
    world
        .progress
        .rules
        .upgrades
        .get(part)
        .map_or_else(|| part.to_string(), |u| u.name.clone())
}

/// The preview of `held` let go on `over`.
fn preview(world: &WorldState, history: &History, held: &Held, over: &Over) -> Option<HeldPreview> {
    let name = part_name(world, &held.part);
    if held.from == Source::Socket {
        let socket = socket_of(world, &held.part)?;
        let ship = galaxy_contract::fit::flagship(world)?;
        let mut after = world.player.fittings.clone();
        after.remove(&held.part);
        let request = ServiceRequest::Remove {
            upgrade: held.part.clone(),
        };
        let refusal = galaxy_progress::services::check(world, history, &request)
            .err()
            .map(|r| refusal_line(world, &request, &r));
        return Some(HeldPreview {
            part: held.part.clone(),
            name,
            socket,
            removing: true,
            before: hullwright::stats(world)?,
            after: hullwright::stats_with(world, &after)?,
            budget_before: fit::budget(world, &ship.hull, &world.player.fittings),
            budget_after: fit::budget(world, &ship.hull, &after),
            swaps_out: None,
            request,
            refusal,
        });
    }
    let p: Preview = hullwright::preview(world, history, &held.part)?;
    let _ = over;
    Some(HeldPreview {
        part: p.part,
        name,
        socket: p.socket,
        removing: false,
        before: p.before,
        after: p.after,
        budget_before: p.budget_before,
        budget_after: p.budget_after,
        swaps_out: p.swaps_out,
        refusal: p
            .refusal
            .as_ref()
            .map(|r| refusal_line(world, &p.request, r)),
        request: p.request,
    })
}

/// What the keeper says about an empty socket clicked with nothing in hand.
fn empty_socket_line(world: &WorldState, station: &StationId, s: &SocketId) -> String {
    let name = socket_name(world, s).to_lowercase();
    let works = world
        .progress
        .rules
        .fit
        .wharves
        .get(station)
        .is_some_and(|w| w.sockets.contains(s));
    if works {
        format!(
            "The {name} is free. Drag a part from the tray onto it, or click a part and then the socket."
        )
    } else {
        format!("I don't work on the {name} here.")
    }
}

/// A refusal as the keeper says it: first person, what stands in the way and, where
/// there is one, what to do about it.
pub fn refusal_line(
    world: &WorldState,
    req: &ServiceRequest,
    r: &galaxy_progress::services::Refusal,
) -> String {
    use galaxy_progress::hullwright::FitRefusal as F;
    use galaxy_progress::services::Refusal as R;
    match r {
        R::Fit(F::OverPower { need, cap, hull }) => format!(
            "That would draw {need} power, and the {hull} only has {cap}. Take something off first."
        ),
        R::Fit(F::NoWharf { socket }) => format!("I don't work on the {socket} here."),
        R::Fit(F::Module) => "I don't take controls off. They stay with you.".into(),
        R::Fit(F::HoldFull { holds, room }) => format!(
            "Your hold would take {room}, and you carry {holds}. Sell or deliver some first."
        ),
        R::Locked => "I can't sell you that yet.".into(),
        _ => crate::trades::service_refusal(world, req, r),
    }
}

/// The keeper's word once a request from the room went through, if the talk at the
/// dock doesn't say it already: a fit (bought, or made and fitted) has the keeper's own
/// bubble from the core pack's `fitted` script.
pub fn done_line(world: &WorldState, req: &ServiceRequest) -> Option<String> {
    let ship = galaxy_contract::fit::flagship(world)
        .map_or_else(|| "your ship".to_owned(), |s| format!("the {}", s.name));
    let place = |u: &UpgradeId| {
        let place = socket_of(world, u)
            .and_then(|s| world.progress.rules.fit.sockets.get(&s))
            .map_or("hull", |d| match d.place.as_str() {
                "nose" => "nose",
                "tail" => "tail",
                "belly" => "belly",
                _ => "back",
            });
        format!("her {place}")
    };
    let name = |u: &UpgradeId| part_name(world, u).to_lowercase();
    Some(match req {
        ServiceRequest::Fit { .. }
        | ServiceRequest::Make { fit: true, .. }
        | ServiceRequest::Build { .. }
        | ServiceRequest::Prestige { .. } => return None,
        ServiceRequest::Refit { upgrade } => format!(
            "Out of the locker and back on {}: your {}.",
            place(upgrade),
            name(upgrade)
        ),
        ServiceRequest::Remove { upgrade } => format!(
            "I've put your {} in {ship}'s locker. Nothing's lost: drag it back onto a socket whenever you like.",
            name(upgrade)
        ),
        ServiceRequest::Make { upgrade, .. } => format!(
            "Made from what you brought: your {}, waiting in {ship}'s locker.",
            name(upgrade)
        ),
        ServiceRequest::Order { upgrade } => {
            let from = world
                .progress
                .rules
                .upgrades
                .get(upgrade)
                .and_then(|u| match &u.source {
                    galaxy_contract::fit::PartSource::Friend { station, .. } => {
                        world.stations.get(station).map(|s| s.name.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| "their station".into());
            format!("I've sent word. It comes after your next docking at {from}.")
        }
    })
}

/// Sends a request from the room, as the window and the key scripts both do: the
/// desk checks it at once, and a refused one plays no fitting animation.
pub fn send(session: &mut crate::session::Session, ui: &mut WharfUi, req: ServiceRequest) {
    ui.sending(&req);
    session.request_service(req);
    if session.services.notice.is_some() {
        ui.refused();
    }
}

/// After a tick: a fit the desk refused when it settled plays no animation either.
pub fn settle(session: &crate::session::Session, ui: &mut WharfUi) {
    let desk = &session.services;
    if ui.fitting.is_some() && !desk.is_pending() && desk.notice.is_some() && desk.fitted.is_none()
    {
        ui.refused();
    }
}

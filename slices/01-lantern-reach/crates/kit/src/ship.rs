//! Ship hulls as recipes: the parameters a hull mesh is built from, its named sockets,
//! and the look of what is fitted on them.
//!
//! Render-free, like [`crate::station`]: the render crate builds the meshes, and the app
//! and UI can place sockets without it.
//!
//! Local axes: +X is forward (the nose), +Y is left, +Z is up. The hull is centred on the
//! origin.

use crate::color::Color;
use crate::palette;
use glam::Vec3;
use serde::{Deserialize, Serialize};

/// The three hulls in the slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HullClass {
    /// Tier 0: a small, round, friendly first ship.
    Wren,
    /// Hauler that pairs with automation: boxy body, trailing cargo pods.
    Tandem,
    /// Scout with long price reach: slender body, long wings, a sensor mast.
    Heron,
}

/// Paint for a hull.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HullColors {
    pub body: Color,
    pub nose: Color,
    pub trim: Color,
    pub canopy: Color,
    pub engine: Color,
}

impl Default for HullColors {
    fn default() -> Self {
        Self {
            body: palette::CORAL,
            nose: palette::HULL_CREAM,
            trim: palette::MUSTARD,
            canopy: palette::GLASS,
            engine: palette::LANTERN,
        }
    }
}

/// The four places a part attaches to a hull, as the hullwright names them: the drill
/// mount on the nose, utility behind the cockpit, the hold on the belly (its parts hang
/// off the sides) and the engine at the tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Socket {
    Nose,
    Utility,
    Belly,
    Tail,
}

impl Socket {
    pub const ALL: [Socket; 4] = [Socket::Nose, Socket::Utility, Socket::Belly, Socket::Tail];

    /// The hullwright's name for it.
    pub fn label(self) -> &'static str {
        match self {
            Socket::Nose => "Drill mount",
            Socket::Utility => "Utility",
            Socket::Belly => "Hold",
            Socket::Tail => "Engine",
        }
    }
}

/// What a fitted part looks like from outside. Control-rack modules have none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookId {
    /// A glass lens ring on the nose.
    Lens,
    /// A long drill on the nose.
    LongDrill,
    /// Hold pods on the sides.
    Pods,
    /// Swift fins sweeping back from the tail.
    Fins,
    /// A small lamp on the utility socket.
    SeamLamp,
}

impl LookId {
    pub const ALL: [LookId; 5] = [
        LookId::Lens,
        LookId::LongDrill,
        LookId::Pods,
        LookId::Fins,
        LookId::SeamLamp,
    ];

    /// The id as data writes it: `lens`, `long_drill`, `pods`, `fins`, `seam_lamp`.
    pub fn as_str(self) -> &'static str {
        match self {
            LookId::Lens => "lens",
            LookId::LongDrill => "long_drill",
            LookId::Pods => "pods",
            LookId::Fins => "fins",
            LookId::SeamLamp => "seam_lamp",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.as_str() == id)
    }

    /// The socket the part is built for.
    pub fn socket(self) -> Socket {
        match self {
            LookId::Lens | LookId::LongDrill => Socket::Nose,
            LookId::Pods => Socket::Belly,
            LookId::Fins => Socket::Tail,
            LookId::SeamLamp => Socket::Utility,
        }
    }
}

/// What a ship wears: at most one look per socket. Plain data, ordered, so it keys a
/// mesh cache.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(default)]
pub struct ShipFitLook {
    pub nose: Option<LookId>,
    pub utility: Option<LookId>,
    pub belly: Option<LookId>,
    pub tail: Option<LookId>,
}

impl ShipFitLook {
    /// From `(socket, look)` pairs. A later pair on the same socket replaces an earlier.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (Socket, LookId)>) -> Self {
        let mut look = Self::default();
        for (socket, id) in pairs {
            look.set(socket, Some(id));
        }
        look
    }

    pub fn get(&self, socket: Socket) -> Option<LookId> {
        match socket {
            Socket::Nose => self.nose,
            Socket::Utility => self.utility,
            Socket::Belly => self.belly,
            Socket::Tail => self.tail,
        }
    }

    pub fn set(&mut self, socket: Socket, look: Option<LookId>) {
        match socket {
            Socket::Nose => self.nose = look,
            Socket::Utility => self.utility = look,
            Socket::Belly => self.belly = look,
            Socket::Tail => self.tail = look,
        }
    }

    /// This look with `look` fitted on `socket`: a preview of a part before it is fitted.
    pub fn with(mut self, socket: Socket, look: LookId) -> Self {
        self.set(socket, Some(look));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.pairs().next().is_none()
    }

    /// The fitted `(socket, look)` pairs, nose to tail.
    pub fn pairs(&self) -> impl Iterator<Item = (Socket, LookId)> + '_ {
        Socket::ALL
            .into_iter()
            .filter_map(|s| self.get(s).map(|l| (s, l)))
    }
}

/// A hull's sockets, each as a fraction of the hull's size: `[x of the length, y of the
/// width, z of the height]` from the hull's centre. Scaling a hull moves its sockets
/// with it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HullSockets {
    pub nose: [f32; 3],
    pub utility: [f32; 3],
    pub belly: [f32; 3],
    pub tail: [f32; 3],
}

impl Default for HullSockets {
    fn default() -> Self {
        Self::wren()
    }
}

impl HullSockets {
    pub fn wren() -> Self {
        Self {
            nose: [0.5, 0.0, 0.0],
            utility: [-0.06, -0.12, 0.5],
            belly: [-0.24, 0.0, 0.0],
            tail: [-0.46, 0.0, 0.0],
        }
    }

    pub fn tandem() -> Self {
        Self {
            nose: [0.5, 0.0, 0.0],
            utility: [-0.08, -0.14, 0.55],
            belly: [-0.26, 0.0, 0.0],
            tail: [-0.46, 0.0, 0.0],
        }
    }

    pub fn heron() -> Self {
        Self {
            nose: [0.5, 0.0, 0.0],
            utility: [-0.06, -0.16, 0.5],
            belly: [-0.22, 0.0, 0.0],
            tail: [-0.46, 0.0, 0.0],
        }
    }

    pub fn get(&self, socket: Socket) -> [f32; 3] {
        match socket {
            Socket::Nose => self.nose,
            Socket::Utility => self.utility,
            Socket::Belly => self.belly,
            Socket::Tail => self.tail,
        }
    }
}

/// Parameters for a hull mesh. Lengths are world units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HullParams {
    pub class: HullClass,
    /// Nose to tail.
    pub length: f32,
    /// Widest point of the body.
    pub width: f32,
    /// Body thickness.
    pub height: f32,
    /// 0 is a blunt bean, 1 a sharp beak.
    pub nose: f32,
    /// Wing tip to wing tip (0 for no wings).
    pub wing_span: f32,
    /// How far back the wings sweep, `0..=1`.
    pub wing_sweep: f32,
    /// 1 to 3 engines.
    pub engines: u32,
    /// Cargo pods trailing behind (the Tandem's signature).
    pub cargo_pods: u32,
    /// Tail fin height (0 for none).
    pub fin: f32,
    /// Forward sensor mast length (the Heron's signature; 0 for none).
    pub mast: f32,
    /// Facets around the body; fewer reads as more hand-cut.
    pub facets: u32,
    pub colors: HullColors,
    /// Faction sigil on the back. Empty draws none.
    /// `lantern_kettle`, `toll_key` or `open_lamp`.
    #[serde(default = "empty_mark", skip_serializing_if = "String::is_empty")]
    pub mark: String,
    /// Where fitted parts attach ([`HullParams::socket`]).
    pub sockets: HullSockets,
}

fn empty_mark() -> String {
    String::new()
}

impl Default for HullParams {
    fn default() -> Self {
        Self::wren()
    }
}

impl HullParams {
    pub fn wren() -> Self {
        Self {
            class: HullClass::Wren,
            length: 6.0,
            width: 3.2,
            height: 2.0,
            nose: 0.35,
            wing_span: 6.4,
            wing_sweep: 0.45,
            engines: 1,
            cargo_pods: 0,
            fin: 1.1,
            mast: 0.0,
            facets: 8,
            colors: HullColors::default(),
            mark: "lantern_kettle".into(),
            sockets: HullSockets::wren(),
        }
    }

    pub fn tandem() -> Self {
        Self {
            class: HullClass::Tandem,
            length: 8.0,
            width: 3.6,
            height: 2.4,
            nose: 0.15,
            wing_span: 5.6,
            wing_sweep: 0.2,
            engines: 2,
            cargo_pods: 2,
            fin: 0.8,
            mast: 0.0,
            facets: 6,
            colors: HullColors {
                body: palette::TEAL,
                nose: palette::HULL_CREAM,
                trim: palette::TERRACOTTA,
                ..HullColors::default()
            },
            mark: String::new(),
            sockets: HullSockets::tandem(),
        }
    }

    pub fn heron() -> Self {
        Self {
            class: HullClass::Heron,
            length: 9.0,
            width: 2.4,
            height: 1.6,
            nose: 0.9,
            wing_span: 10.0,
            wing_sweep: 0.75,
            engines: 1,
            cargo_pods: 0,
            fin: 1.4,
            mast: 3.0,
            facets: 8,
            colors: HullColors {
                body: palette::HULL_CREAM,
                nose: palette::NEBULA_VIOLET,
                trim: palette::TEAL,
                ..HullColors::default()
            },
            mark: String::new(),
            sockets: HullSockets::heron(),
        }
    }

    pub fn for_class(class: HullClass) -> Self {
        match class {
            HullClass::Wren => Self::wren(),
            HullClass::Tandem => Self::tandem(),
            HullClass::Heron => Self::heron(),
        }
    }

    /// Clamps every field into a range that builds a sane mesh.
    pub fn sanitized(&self) -> Self {
        let f = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        let s = |p: [f32; 3]| p.map(|v| f(v, -1.0, 1.0));
        Self {
            class: self.class,
            length: f(self.length, 1.0, 60.0),
            width: f(self.width, 0.4, 30.0),
            height: f(self.height, 0.3, 20.0),
            nose: f(self.nose, 0.0, 1.0),
            wing_span: f(self.wing_span, 0.0, 80.0),
            wing_sweep: f(self.wing_sweep, 0.0, 1.0),
            engines: self.engines.clamp(1, 3),
            cargo_pods: self.cargo_pods.min(4),
            fin: f(self.fin, 0.0, 20.0),
            mast: f(self.mast, 0.0, 40.0),
            facets: self.facets.clamp(5, 16),
            colors: self.colors,
            mark: self.mark.clone(),
            sockets: HullSockets {
                nose: s(self.sockets.nose),
                utility: s(self.sockets.utility),
                belly: s(self.sockets.belly),
                tail: s(self.sockets.tail),
            },
        }
    }

    /// Where `socket` sits on the hull, in mesh units (sanitized).
    pub fn socket(&self, socket: Socket) -> Vec3 {
        let p = self.sanitized();
        let [x, y, z] = p.sockets.get(socket);
        Vec3::new(x * p.length, y * p.width, z * p.height)
    }

    /// How far the nose reaches forward with `look` fitted, in mesh units: a long drill
    /// reaches past the hull's own nose. The mining beam starts here.
    pub fn nose_reach(&self, look: &ShipFitLook) -> f32 {
        let p = self.sanitized();
        let own = p.length * 0.5;
        if look.nose == Some(LookId::LongDrill) {
            own.max(self.socket(Socket::Nose).x + long_drill_length(&p))
        } else {
            own
        }
    }
}

/// How long a long drill is on this hull, mesh units.
pub fn long_drill_length(p: &HullParams) -> f32 {
    (p.length * 0.55).max(1.6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_parse_their_data_ids() {
        for l in LookId::ALL {
            assert_eq!(LookId::parse(l.as_str()), Some(l));
            let json = serde_json::to_string(&l).unwrap();
            assert_eq!(json, format!("\"{}\"", l.as_str()));
        }
        assert_eq!(LookId::parse("rack"), None);
    }

    #[test]
    fn a_fit_look_is_built_from_pairs() {
        let look = ShipFitLook::from_pairs([
            (Socket::Nose, LookId::Lens),
            (Socket::Belly, LookId::Pods),
            (Socket::Nose, LookId::LongDrill),
        ]);
        assert_eq!(look.nose, Some(LookId::LongDrill), "the later pair wins");
        assert_eq!(look.belly, Some(LookId::Pods));
        assert_eq!(look.utility, None);
        assert_eq!(
            look.pairs().collect::<Vec<_>>(),
            vec![
                (Socket::Nose, LookId::LongDrill),
                (Socket::Belly, LookId::Pods)
            ]
        );
        assert!(ShipFitLook::default().is_empty());
        let preview = look.with(Socket::Tail, LookId::Fins);
        assert_eq!(preview.tail, Some(LookId::Fins));
        assert_eq!(look.tail, None, "a preview leaves the fit alone");
    }

    #[test]
    fn sockets_sit_nose_to_tail_on_every_hull() {
        for p in [
            HullParams::wren(),
            HullParams::tandem(),
            HullParams::heron(),
        ] {
            let x = Socket::ALL.map(|s| p.socket(s).x);
            assert!(
                x[0] > x[1] && x[1] > x[3] && x[2] > x[3],
                "{:?}: {x:?}",
                p.class
            );
            assert!((x[0] - p.length * 0.5).abs() < 1e-4);
            // The utility socket is on top of the hull, the others on its midline.
            assert!(p.socket(Socket::Utility).z > 0.0);
        }
        // Sockets scale with the hull.
        let big = HullParams {
            length: 12.0,
            ..HullParams::wren()
        };
        assert_eq!(
            big.socket(Socket::Tail).x,
            HullParams::wren().socket(Socket::Tail).x * 2.0
        );
    }

    #[test]
    fn a_long_drill_reaches_past_the_nose() {
        let p = HullParams::wren();
        let none = p.nose_reach(&ShipFitLook::default());
        assert_eq!(none, p.length * 0.5);
        let lens = ShipFitLook::default().with(Socket::Nose, LookId::Lens);
        assert_eq!(p.nose_reach(&lens), none);
        let drill = ShipFitLook::default().with(Socket::Nose, LookId::LongDrill);
        assert!(p.nose_reach(&drill) > none + 1.0);
    }

    #[test]
    fn older_params_without_sockets_load() {
        let p: HullParams = serde_json::from_str(r#"{"class": "heron", "length": 9.0}"#).unwrap();
        assert_eq!(p.sockets, HullSockets::wren());
        let wild = HullParams {
            sockets: HullSockets {
                nose: [f32::NAN, 9.0, -9.0],
                ..HullSockets::wren()
            },
            ..HullParams::wren()
        };
        assert!(wild.socket(Socket::Nose).is_finite());
    }
}

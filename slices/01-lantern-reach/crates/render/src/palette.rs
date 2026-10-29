//! The Lantern Reach palette: warm, saturated and a little retro. No gray/blue chrome.
//!
//! Constants are in linear space. Each doc comment gives the sRGB hex it was picked as
//! (see [`Color::hex`]).

use crate::color::Color;

macro_rules! swatch {
    ($($(#[$m:meta])* $name:ident = $r:expr, $g:expr, $b:expr;)*) => {
        $($(#[$m])* pub const $name: Color = Color::linear($r, $g, $b, 1.0);)*
    };
}

// Values are sRGB hex converted to linear (r, g, b). The hex is in each comment.
swatch! {
    /// #2a1a2e: the ink for outlines, borders and text.
    INK = 0.0232, 0.0103, 0.0273;
    /// #f7e8c8: panel parchment.
    PARCHMENT = 0.9301, 0.8070, 0.5776;
    /// #fff4dd: light text on dark.
    CREAM = 1.0000, 0.9047, 0.7231;
    /// #1b1430: deep space behind everything.
    SPACE_DEEP = 0.0110, 0.0070, 0.0296;
    /// #3a2352: the lighter top of the space gradient.
    SPACE_HIGH = 0.0423, 0.0168, 0.0844;
    /// #c2577a: rose nebula.
    NEBULA_ROSE = 0.5395, 0.0953, 0.1946;
    /// #e8905a: amber nebula.
    NEBULA_AMBER = 0.8070, 0.2789, 0.1022;
    /// #2f8f8a: teal nebula.
    NEBULA_TEAL = 0.0284, 0.2747, 0.2542;
    /// #6b4aa0: violet nebula.
    NEBULA_VIOLET = 0.1470, 0.0685, 0.3515;
    /// #efdcb4: station hull cream.
    HULL_CREAM = 0.8632, 0.7157, 0.4564;
    /// #d9774b: terracotta trim.
    TERRACOTTA = 0.6939, 0.1845, 0.0704;
    /// #3fa7a0: teal accent.
    TEAL = 0.0497, 0.3864, 0.3515;
    /// #f2b43c: mustard accent.
    MUSTARD = 0.8879, 0.4564, 0.0452;
    /// #c98f4a: brass fittings.
    BRASS = 0.5841, 0.2747, 0.0685;
    /// #8fbf7a: sage panels.
    SAGE = 0.2747, 0.5210, 0.1946;
    /// #5cbf5a: greenhouse leaves.
    LEAF = 0.1070, 0.5210, 0.1022;
    /// #ff8fa3: blossoms.
    BLOSSOM = 1.0000, 0.2747, 0.3663;
    /// #ffcf6b: lantern glow.
    LANTERN = 1.0000, 0.6240, 0.1470;
    /// #ffe29a: lit windows.
    WINDOW = 1.0000, 0.7605, 0.3231;
    /// #ee7a5a: Wren coral.
    CORAL = 0.8550, 0.1946, 0.1022;
    /// #7fd6e0: canopy glass.
    GLASS = 0.2122, 0.6724, 0.7454;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swatches_match_their_hex() {
        for (c, hex) in [
            (INK, "#2a1a2e"),
            (PARCHMENT, "#f7e8c8"),
            (SPACE_DEEP, "#1b1430"),
            (NEBULA_ROSE, "#c2577a"),
            (TERRACOTTA, "#d9774b"),
            (LANTERN, "#ffcf6b"),
            (GLASS, "#7fd6e0"),
            (CORAL, "#ee7a5a"),
            (SAGE, "#8fbf7a"),
        ] {
            assert_eq!(c.to_hex(), hex);
        }
    }
}

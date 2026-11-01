//! The engine's default colours, used until a module's style says otherwise
//! ([`crate::style`]). Linear values; the comment gives the sRGB hex.

use galaxy_mesh::Color;

/// #2a1a2e: ink for outlines, borders and text.
pub const INK: Color = Color::linear(0.0232, 0.0103, 0.0273, 1.0);
/// #f7e8c8: panel paper.
pub const PAPER: Color = Color::linear(0.9301, 0.8070, 0.5776, 1.0);
/// #4e3672: the top of the sky gradient.
pub const SKY_TOP: Color = Color::linear(0.0762, 0.0369, 0.1683, 1.0);
/// #2a1f48: the bottom of the sky gradient, and the clear colour.
pub const SKY_BOTTOM: Color = Color::linear(0.0232, 0.0137, 0.0648, 1.0);
/// #c2577a, #6b4aa0, #2f8f8a, #e8905a: cloud colours.
pub const CLOUD_ROSE: Color = Color::linear(0.5395, 0.0953, 0.1946, 1.0);
pub const CLOUD_VIOLET: Color = Color::linear(0.1470, 0.0685, 0.3515, 1.0);
pub const CLOUD_TEAL: Color = Color::linear(0.0284, 0.2747, 0.2542, 1.0);
pub const CLOUD_AMBER: Color = Color::linear(0.8070, 0.2789, 0.1022, 1.0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_their_hex() {
        for (c, hex) in [
            (INK, "#2a1a2e"),
            (PAPER, "#f7e8c8"),
            (SKY_TOP, "#4e3672"),
            (SKY_BOTTOM, "#2a1f48"),
            (CLOUD_ROSE, "#c2577a"),
            (CLOUD_VIOLET, "#6b4aa0"),
            (CLOUD_TEAL, "#2f8f8a"),
            (CLOUD_AMBER, "#e8905a"),
        ] {
            assert_eq!(c.to_hex(), hex);
        }
    }
}

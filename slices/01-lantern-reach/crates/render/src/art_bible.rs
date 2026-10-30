use crate::scene::Material;
pub use galaxy_kit::art_bible::*;
pub trait ArtMaterial {
    fn material(&self, role: InkRole) -> Material;
}
impl ArtMaterial for ArtBible {
    /// Cel material for a role: silhouette weight and hatch from this bible.
    fn material(&self, role: InkRole) -> Material {
        let (px, hatch) = match role {
            InkRole::Station => (mid(self.ink.station_px), self.hatch.station),
            InkRole::Ship => (mid(self.ink.ship_px), self.hatch.ship),
            InkRole::Rock => (mid(self.ink.rock_px), self.hatch.rock),
            InkRole::Prop => (mid(self.ink.prop_px), self.hatch.prop),
        };
        Material {
            outline: self.ink.color,
            outline_px: px,
            hatch,
            ..Material::default()
        }
    }
}
fn mid(r: [f32; 2]) -> f32 {
    (r[0] + r[1]) * 0.5
}

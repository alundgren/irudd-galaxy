//! Pictures for the tools: 8-bit sRGB RGBA images read from and written to PNG, sampled
//! between pixels, and drawn on with lines, boxes and a small built-in font, so every
//! report picture can carry its own labels and timestamps.

use std::path::Path;

pub type Rgba = [u8; 4];

pub const BLACK: Rgba = [0, 0, 0, 255];
pub const WHITE: Rgba = [255, 255, 255, 255];
pub const PAPER: Rgba = [24, 26, 30, 255];
pub const GRID: Rgba = [60, 64, 72, 255];
pub const TEXT: Rgba = [220, 224, 230, 255];
pub const FLAG: Rgba = [255, 92, 72, 255];
pub const ACCENT: Rgba = [255, 207, 107, 255];
pub const CALM: Rgba = [110, 200, 160, 255];

/// An 8-bit sRGB RGBA image, row-major from the top left.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, fill: Rgba) -> Image {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            pixels.extend_from_slice(&fill);
        }
        Image {
            width,
            height,
            pixels,
        }
    }

    pub fn get(&self, x: u32, y: u32) -> Rgba {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    pub fn set(&mut self, x: i64, y: i64, c: Rgba) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        self.pixels[i..i + 4].copy_from_slice(&c);
    }

    /// Paints `c` over the pixel with `c`'s alpha times `cover`.
    pub fn blend(&mut self, x: i64, y: i64, c: Rgba, cover: f32) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let a = (f32::from(c[3]) / 255.0 * cover).clamp(0.0, 1.0);
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        for (old, new) in self.pixels[i..i + 3].iter_mut().zip(c) {
            let o = f32::from(*old);
            *old = (o + (f32::from(new) - o) * a).round() as u8;
        }
        self.pixels[i + 3] = 255;
    }

    /// Brightness 0 to 1 (Rec. 709 weights on the sRGB values).
    pub fn luma(&self, x: u32, y: u32) -> f32 {
        let p = self.get(x, y);
        (0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])) / 255.0
    }

    /// The image as brightness.
    pub fn gray(&self) -> Gray {
        let mut data = Vec::with_capacity((self.width * self.height) as usize);
        for y in 0..self.height {
            for x in 0..self.width {
                data.push(self.luma(x, y));
            }
        }
        Gray {
            width: self.width,
            height: self.height,
            data,
        }
    }

    /// The colour at `(x, y)` between pixel centres (pixel `i` is centred on `i + 0.5`);
    /// none outside the image.
    pub fn sample(&self, x: f32, y: f32) -> Option<[f32; 4]> {
        let (fx, fy) = (x - 0.5, y - 0.5);
        if !(fx >= -0.5
            && fy >= -0.5
            && fx <= self.width as f32 - 0.5
            && fy <= self.height as f32 - 0.5)
        {
            return None;
        }
        let x0 = fx.floor().clamp(0.0, (self.width - 1) as f32) as u32;
        let y0 = fy.floor().clamp(0.0, (self.height - 1) as f32) as u32;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let tx = (fx - x0 as f32).clamp(0.0, 1.0);
        let ty = (fy - y0 as f32).clamp(0.0, 1.0);
        let (a, b, c, d) = (
            self.get(x0, y0),
            self.get(x1, y0),
            self.get(x0, y1),
            self.get(x1, y1),
        );
        let mut out = [0.0; 4];
        for k in 0..4 {
            let top = f32::from(a[k]) + (f32::from(b[k]) - f32::from(a[k])) * tx;
            let bottom = f32::from(c[k]) + (f32::from(d[k]) - f32::from(c[k])) * tx;
            out[k] = top + (bottom - top) * ty;
        }
        Some(out)
    }

    /// A `w` x `h` piece from `(x, y)`; outside the image is `fill`.
    pub fn crop(&self, x: i64, y: i64, w: u32, h: u32, fill: Rgba) -> Image {
        let mut out = Image::new(w, h, fill);
        for j in 0..h {
            for i in 0..w {
                let (sx, sy) = (x + i64::from(i), y + i64::from(j));
                if sx >= 0 && sy >= 0 && sx < i64::from(self.width) && sy < i64::from(self.height) {
                    out.set(i64::from(i), i64::from(j), self.get(sx as u32, sy as u32));
                }
            }
        }
        out
    }

    /// The image shrunk or grown to `w` x `h`, averaging the pixels each new one covers.
    pub fn resized(&self, w: u32, h: u32) -> Image {
        let mut out = Image::new(w.max(1), h.max(1), BLACK);
        let sx = self.width as f32 / out.width as f32;
        let sy = self.height as f32 / out.height as f32;
        for j in 0..out.height {
            for i in 0..out.width {
                let x0 = (i as f32 * sx) as u32;
                let y0 = (j as f32 * sy) as u32;
                let x1 = (((i + 1) as f32 * sx).ceil() as u32).clamp(x0 + 1, self.width);
                let y1 = (((j + 1) as f32 * sy).ceil() as u32).clamp(y0 + 1, self.height);
                let mut sum = [0u32; 4];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = self.get(x, y);
                        for k in 0..4 {
                            sum[k] += u32::from(p[k]);
                        }
                    }
                }
                let n = (x1 - x0) * (y1 - y0);
                out.set(
                    i64::from(i),
                    i64::from(j),
                    [
                        (sum[0] / n) as u8,
                        (sum[1] / n) as u8,
                        (sum[2] / n) as u8,
                        255,
                    ],
                );
            }
        }
        out
    }

    /// Copies `other` in with its top left at `(x, y)`.
    pub fn blit(&mut self, other: &Image, x: i64, y: i64) {
        for j in 0..other.height {
            for i in 0..other.width {
                self.set(x + i64::from(i), y + i64::from(j), other.get(i, j));
            }
        }
    }

    pub fn fill_rect(&mut self, x: i64, y: i64, w: u32, h: u32, c: Rgba) {
        for j in 0..i64::from(h) {
            for i in 0..i64::from(w) {
                self.blend(x + i, y + j, c, 1.0);
            }
        }
    }

    pub fn outline_rect(&mut self, x: i64, y: i64, w: u32, h: u32, c: Rgba) {
        let (w, h) = (i64::from(w), i64::from(h));
        for i in 0..w {
            self.set(x + i, y, c);
            self.set(x + i, y + h - 1, c);
        }
        for j in 0..h {
            self.set(x, y + j, c);
            self.set(x + w - 1, y + j, c);
        }
    }

    /// A line one pixel wide, smoothed along its length.
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgba) {
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).ceil().max(1.0) as i64;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let x = x0 + (x1 - x0) * t;
            let y = y0 + (y1 - y0) * t;
            self.blend(x.round() as i64, y.round() as i64, c, 1.0);
        }
    }

    /// A filled round dot.
    pub fn dot(&mut self, x: f32, y: f32, r: f32, c: Rgba) {
        let reach = r.ceil() as i64 + 1;
        for j in -reach..=reach {
            for i in -reach..=reach {
                let (px, py) = (x.floor() as i64 + i, y.floor() as i64 + j);
                let d = ((px as f32 + 0.5 - x).powi(2) + (py as f32 + 0.5 - y).powi(2)).sqrt();
                let cover = (r + 0.5 - d).clamp(0.0, 1.0);
                if cover > 0.0 {
                    self.blend(px, py, c, cover);
                }
            }
        }
    }

    /// Text in the built-in 5x7 font, `size` pixels per font pixel, top left at `(x, y)`.
    /// Returns the width drawn.
    pub fn text(&mut self, x: i64, y: i64, text: &str, size: u32, c: Rgba) -> i64 {
        let s = i64::from(size.max(1));
        let mut cx = x;
        for ch in text.chars() {
            let rows = glyph(ch);
            for (j, row) in rows.iter().enumerate() {
                for i in 0..5 {
                    if row & (0b10000 >> i) != 0 {
                        for dy in 0..s {
                            for dx in 0..s {
                                self.set(cx + i * s + dx, y + j as i64 * s + dy, c);
                            }
                        }
                    }
                }
            }
            cx += 6 * s;
        }
        cx - x
    }

    /// Text on a dark box, so it reads over any picture.
    pub fn label(&mut self, x: i64, y: i64, text: &str, size: u32, c: Rgba) {
        let s = i64::from(size.max(1));
        let w = text.chars().count() as i64 * 6 * s + 2 * s;
        self.fill_rect(x, y, w as u32, (9 * s) as u32, [0, 0, 0, 170]);
        self.text(x + s, y + s, text, size, c);
    }

    pub fn encode_png(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
            enc.set_compression(png::Compression::Fast);
            let mut w = enc.write_header().map_err(|e| e.to_string())?;
            w.write_image_data(&self.pixels)
                .map_err(|e| e.to_string())?;
        }
        Ok(out)
    }

    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        let bytes = self.encode_png()?;
        std::fs::write(path, bytes).map_err(|e| format!("can't write {}: {e}", path.display()))
    }

    /// Reads an 8-bit RGBA or RGB PNG.
    pub fn load_png(path: &Path) -> Result<Image, String> {
        let bad = |e: String| format!("can't read {}: {e}", path.display());
        let file = std::fs::File::open(path).map_err(|e| bad(e.to_string()))?;
        let decoder = png::Decoder::new(std::io::BufReader::new(file));
        let mut reader = decoder.read_info().map_err(|e| bad(e.to_string()))?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| bad("the image is too large".into()))?;
        let mut buf = vec![0; size];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| bad(e.to_string()))?;
        let data = &buf[..info.buffer_size()];
        let pixels = match (info.color_type, info.bit_depth) {
            (png::ColorType::Rgba, png::BitDepth::Eight) => data.to_vec(),
            (png::ColorType::Rgb, png::BitDepth::Eight) => data
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            other => return Err(bad(format!("unsupported PNG layout {other:?}"))),
        };
        Ok(Image {
            width: info.width,
            height: info.height,
            pixels,
        })
    }
}

/// A brightness image, 0 to 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Gray {
    pub width: u32,
    pub height: u32,
    pub data: Vec<f32>,
}

impl Gray {
    pub fn new(width: u32, height: u32) -> Gray {
        Gray {
            width,
            height,
            data: vec![0.0; (width * height) as usize],
        }
    }

    pub fn at(&self, x: u32, y: u32) -> f32 {
        self.data[(y * self.width + x) as usize]
    }

    /// The value at `(x, y)` in pixel units (pixel `i` centred on `i`), clamped at the
    /// edges.
    pub fn sample(&self, x: f32, y: f32) -> f32 {
        let x = x.clamp(0.0, (self.width - 1) as f32);
        let y = y.clamp(0.0, (self.height - 1) as f32);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (tx, ty) = (x - x0 as f32, y - y0 as f32);
        let top = self.at(x0, y0) + (self.at(x1, y0) - self.at(x0, y0)) * tx;
        let bottom = self.at(x0, y1) + (self.at(x1, y1) - self.at(x0, y1)) * tx;
        top + (bottom - top) * ty
    }

    /// Half the size, each pixel the mean of four.
    pub fn half(&self) -> Gray {
        let (w, h) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let mut out = Gray::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.0;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    sum += self.at(
                        (2 * x + dx).min(self.width - 1),
                        (2 * y + dy).min(self.height - 1),
                    );
                }
                out.data[(y * w + x) as usize] = sum / 4.0;
            }
        }
        out
    }
}

/// A colour for `v` from 0 (dark) to 1 (bright yellow): for heat maps.
pub fn heat(v: f32) -> Rgba {
    let stops: [(f32, [f32; 3]); 5] = [
        (0.0, [12.0, 10.0, 30.0]),
        (0.25, [80.0, 20.0, 110.0]),
        (0.5, [190.0, 50.0, 90.0]),
        (0.75, [250.0, 140.0, 40.0]),
        (1.0, [252.0, 250.0, 160.0]),
    ];
    ramp(&stops, v)
}

/// A colour for `t` from 0 (early, blue) to 1 (late, red): for time.
pub fn time_color(t: f32) -> Rgba {
    let stops: [(f32, [f32; 3]); 4] = [
        (0.0, [70.0, 140.0, 255.0]),
        (0.35, [70.0, 220.0, 200.0]),
        (0.7, [250.0, 210.0, 60.0]),
        (1.0, [255.0, 70.0, 60.0]),
    ];
    ramp(&stops, t)
}

fn ramp(stops: &[(f32, [f32; 3])], v: f32) -> Rgba {
    let v = if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    };
    for w in stops.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if v <= b {
            let t = (v - a) / (b - a);
            return [
                (ca[0] + (cb[0] - ca[0]) * t) as u8,
                (ca[1] + (cb[1] - ca[1]) * t) as u8,
                (ca[2] + (cb[2] - ca[2]) * t) as u8,
                255,
            ];
        }
    }
    let c = stops[stops.len() - 1].1;
    [c[0] as u8, c[1] as u8, c[2] as u8, 255]
}

/// Rows of a 5x7 glyph, the high bit of the five on the left. Letters are capitals
/// whichever case is asked for; an unknown character is a box.
fn glyph(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        '2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        '3' => [0x1f, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x1e, 0x01, 0x01, 0x11, 0x0e],
        '6' => [0x06, 0x08, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x02, 0x0c],
        'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        'C' => [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e],
        'D' => [0x1c, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1c],
        'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        'G' => [0x0e, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0f],
        'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'I' => [0x0e, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0c],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        'M' => [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        'Q' => [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
        'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a],
        'X' => [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x11, 0x0a, 0x04, 0x04, 0x04],
        'Z' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f],
        ' ' => [0; 7],
        '.' => [0, 0, 0, 0, 0, 0x0c, 0x0c],
        ':' => [0, 0x0c, 0x0c, 0, 0x0c, 0x0c, 0],
        '-' => [0, 0, 0, 0x1f, 0, 0, 0],
        '+' => [0, 0x04, 0x04, 0x1f, 0x04, 0x04, 0],
        '/' => [0, 0x01, 0x02, 0x04, 0x08, 0x10, 0],
        '=' => [0, 0, 0x1f, 0, 0x1f, 0, 0],
        '%' => [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03],
        ',' => [0, 0, 0, 0, 0x0c, 0x04, 0x08],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '_' => [0, 0, 0, 0, 0, 0, 0x1f],
        '#' => [0x0a, 0x0a, 0x1f, 0x0a, 0x1f, 0x0a, 0x0a],
        '>' => [0x08, 0x04, 0x02, 0x01, 0x02, 0x04, 0x08],
        '<' => [0x02, 0x04, 0x08, 0x10, 0x08, 0x04, 0x02],
        '|' => [0x04; 7],
        '\'' => [0x04, 0x04, 0x08, 0, 0, 0, 0],
        _ => [0x1f, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1f],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut img = Image::new(8, 4, BLACK);
        img.set(3, 2, [10, 20, 30, 255]);
        let path = dir.path().join("a.png");
        img.save_png(&path).unwrap();
        assert_eq!(Image::load_png(&path).unwrap(), img);
    }

    #[test]
    fn sampling_between_pixels_blends_them() {
        let mut img = Image::new(2, 1, BLACK);
        img.set(1, 0, WHITE);
        let mid = img.sample(1.0, 0.5).unwrap();
        assert!((mid[0] - 127.5).abs() < 0.01, "{mid:?}");
        assert_eq!(img.sample(0.5, 0.5).unwrap()[0], 0.0);
        assert!(img.sample(-1.0, 0.5).is_none());
    }

    #[test]
    fn text_draws_in_the_box_it_reports() {
        let mut img = Image::new(40, 10, BLACK);
        let w = img.text(1, 1, "t=1", 1, WHITE);
        assert_eq!(w, 18);
        assert!((0..40).any(|x| img.get(x, 4) == WHITE));
    }
}

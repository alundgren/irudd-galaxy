//! Module icons as small SVG files: a subset parser and an anti-aliased rasteriser.
//!
//! Icons travel with a module as text, like its scripts, so recordings carry them and
//! audit and film redraw them exactly. The subset covers what icon sets use: `path`
//! (every command, arcs included), `rect`, `circle`, `ellipse`, `line`, `polyline`,
//! `polygon` and `g`, with `fill`, `stroke`, `stroke-width`, `fill-rule`, `opacity` and
//! `transform`. Anything else that would draw is refused with its name, so a check finds
//! it rather than a player seeing a missing shape: other elements, and the `filter`,
//! `clip-path`, `mask` and marker properties that would draw what `<defs>` holds.

/// Largest icon file, in bytes.
pub const MAX_BYTES: usize = 64 * 1024;
/// Most points an icon flattens to.
const MAX_POINTS: usize = 50_000;

/// A parsed icon: filled outlines in its viewBox units.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    /// The viewBox origin and size.
    pub view: [f32; 4],
    pub shapes: Vec<Shape>,
    /// FNV-1a of the source text: the renderer's cache key.
    pub hash: u64,
}

/// One filled shape. Strokes are already turned into outlines.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub contours: Vec<Vec<[f32; 2]>>,
    pub even_odd: bool,
    /// Straight sRGB colour; `currentColor` and unset fills are black.
    pub rgba: [u8; 4],
}

#[derive(Clone, Copy)]
struct Paint {
    fill: Option<[u8; 4]>,
    stroke: Option<[u8; 4]>,
    stroke_width: f32,
    even_odd: bool,
    opacity: f32,
    transform: [f32; 6],
}

impl Icon {
    pub fn parse(text: &str) -> Result<Icon, String> {
        if text.len() > MAX_BYTES {
            return Err(format!("an icon is at most {MAX_BYTES} bytes"));
        }
        let mut view = None;
        let mut stack: Vec<(String, Paint)> = Vec::new();
        let base = Paint {
            fill: Some([0, 0, 0, 255]),
            stroke: None,
            stroke_width: 1.0,
            even_odd: false,
            opacity: 1.0,
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        };
        let mut shapes = Vec::new();
        let mut points = 0;
        let mut skip_depth = 0usize;
        for tag in tags(text)? {
            match tag {
                Tag::Close(name) => {
                    if skip_depth > 0 {
                        skip_depth -= 1;
                        continue;
                    }
                    match stack.pop() {
                        Some((open, _)) if open == name => {}
                        _ => return Err(format!("</{name}> closes nothing")),
                    }
                }
                Tag::Open(name, attrs, empty) => {
                    if skip_depth > 0 {
                        if !empty {
                            skip_depth += 1;
                        }
                        continue;
                    }
                    if matches!(name.as_str(), "title" | "desc" | "metadata" | "defs") {
                        if !empty {
                            skip_depth = 1;
                        }
                        continue;
                    }
                    let parent = stack.last().map(|s| s.1).unwrap_or(base);
                    if name == "svg" {
                        if view.is_some() {
                            return Err("nested <svg> is not supported".into());
                        }
                        view = Some(view_box(&attrs)?);
                    } else if view.is_none() {
                        return Err("an icon starts with <svg>".into());
                    }
                    let paint = paint(&attrs, parent)?;
                    let outline = match name.as_str() {
                        "svg" | "g" => None,
                        "path" => Some(path(attr(&attrs, "d").unwrap_or(""))?),
                        "rect" => Some(rect(&attrs)?),
                        "circle" => {
                            let r = num(&attrs, "r")?;
                            Some(vec![ellipse(num(&attrs, "cx")?, num(&attrs, "cy")?, r, r)])
                        }
                        "ellipse" => Some(vec![ellipse(
                            num(&attrs, "cx")?,
                            num(&attrs, "cy")?,
                            num(&attrs, "rx")?,
                            num(&attrs, "ry")?,
                        )]),
                        "line" => Some(vec![vec![
                            [num(&attrs, "x1")?, num(&attrs, "y1")?],
                            [num(&attrs, "x2")?, num(&attrs, "y2")?],
                        ]]),
                        "polyline" | "polygon" => Some(vec![
                            numbers(attr(&attrs, "points").unwrap_or(""))?
                                .as_chunks::<2>()
                                .0
                                .to_vec(),
                        ]),
                        other => return Err(format!("<{other}> is not supported in icons")),
                    };
                    if let Some(contours) = outline {
                        let closed = !matches!(name.as_str(), "line" | "polyline");
                        let t = paint.transform;
                        let apply = |c: &Vec<[f32; 2]>| -> Vec<[f32; 2]> {
                            c.iter()
                                .map(|p| {
                                    [
                                        t[0] * p[0] + t[2] * p[1] + t[4],
                                        t[1] * p[0] + t[3] * p[1] + t[5],
                                    ]
                                })
                                .collect()
                        };
                        let scale = ((t[0] * t[3] - t[1] * t[2]).abs()).sqrt();
                        if let Some(rgba) = paint.fill.filter(|_| name != "line") {
                            let contours: Vec<_> = contours.iter().map(apply).collect();
                            points += contours.iter().map(Vec::len).sum::<usize>();
                            shapes.push(Shape {
                                contours,
                                even_odd: paint.even_odd,
                                rgba: fade(rgba, paint.opacity),
                            });
                        }
                        if let Some(rgba) = paint.stroke.filter(|_| paint.stroke_width > 0.0) {
                            let mut outline = Vec::new();
                            for c in &contours {
                                stroke(
                                    &apply(c),
                                    closed && name != "path" || is_closed(c),
                                    paint.stroke_width * scale * 0.5,
                                    &mut outline,
                                );
                            }
                            points += outline.iter().map(Vec::len).sum::<usize>();
                            shapes.push(Shape {
                                contours: outline,
                                even_odd: false,
                                rgba: fade(rgba, paint.opacity),
                            });
                        }
                        if points > MAX_POINTS {
                            return Err("the icon is too detailed: simplify its paths".into());
                        }
                    }
                    if !empty {
                        stack.push((name, paint));
                    }
                }
            }
        }
        if let Some((open, _)) = stack.last() {
            return Err(format!("<{open}> is never closed"));
        }
        let view = view.ok_or("an icon starts with <svg>")?;
        if shapes.is_empty() {
            return Err("the icon draws nothing".into());
        }
        let mut hash = 0xcbf29ce484222325u64;
        for b in text.bytes() {
            hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
        }
        Ok(Icon { view, shapes, hash })
    }

    /// Straight-alpha sRGB pixels, `w` by `h`, the viewBox fitted and centred.
    pub fn rasterise(&self, w: u32, h: u32) -> Vec<u8> {
        const S: usize = 4;
        let (w, h) = (w.max(1) as usize, h.max(1) as usize);
        let [vx, vy, vw, vh] = self.view;
        let k = (w as f32 / vw).min(h as f32 / vh);
        let off = [
            (w as f32 - vw * k) * 0.5 - vx * k,
            (h as f32 - vh * k) * 0.5 - vy * k,
        ];
        let mut out = vec![0f32; w * h * 4];
        let mut cover = vec![0u16; w * h];
        for shape in &self.shapes {
            cover.iter_mut().for_each(|c| *c = 0);
            let edges: Vec<[f32; 4]> = shape
                .contours
                .iter()
                .flat_map(|c| {
                    let n = c.len();
                    (0..n).map(move |i| {
                        let (a, b) = (c[i], c[(i + 1) % n]);
                        [
                            a[0] * k + off[0],
                            a[1] * k + off[1],
                            b[0] * k + off[0],
                            b[1] * k + off[1],
                        ]
                    })
                })
                .filter(|e| e[1] != e[3])
                .collect();
            let mut hits: Vec<(f32, i32)> = Vec::new();
            for row in 0..h * S {
                let y = (row as f32 + 0.5) / S as f32;
                hits.clear();
                for e in &edges {
                    let (y0, y1) = (e[1].min(e[3]), e[1].max(e[3]));
                    if y >= y0 && y < y1 {
                        let t = (y - e[1]) / (e[3] - e[1]);
                        hits.push((e[0] + t * (e[2] - e[0]), if e[3] > e[1] { 1 } else { -1 }));
                    }
                }
                hits.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut winding = 0;
                for i in 0..hits.len() {
                    winding += hits[i].1;
                    let inside = if shape.even_odd {
                        (i + 1) % 2 == 1
                    } else {
                        winding != 0
                    };
                    if !inside || i + 1 >= hits.len() {
                        continue;
                    }
                    // Sub-columns whose centres lie in [x0, x1).
                    let first = ((hits[i].0 * S as f32 - 0.5).ceil().max(0.0)) as usize;
                    let last =
                        ((hits[i + 1].0 * S as f32 - 0.5).ceil().max(0.0) as usize).min(w * S);
                    let py = row / S;
                    for sub in first..last {
                        cover[py * w + sub / S] += 1;
                    }
                }
            }
            let [r, g, b, a] = shape.rgba.map(|c| c as f32 / 255.0);
            for (i, &c) in cover.iter().enumerate() {
                let alpha = a * (c as f32 / (S * S) as f32).min(1.0);
                if alpha <= 0.0 {
                    continue;
                }
                // Source over, in straight alpha.
                let d = &mut out[i * 4..i * 4 + 4];
                let oa = alpha + d[3] * (1.0 - alpha);
                for (ch, s) in [r, g, b].into_iter().enumerate() {
                    d[ch] = (s * alpha + d[ch] * d[3] * (1.0 - alpha)) / oa.max(1e-6);
                }
                d[3] = oa;
            }
        }
        out.iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect()
    }
}

fn fade(rgba: [u8; 4], opacity: f32) -> [u8; 4] {
    [
        rgba[0],
        rgba[1],
        rgba[2],
        (rgba[3] as f32 * opacity.clamp(0.0, 1.0)).round() as u8,
    ]
}

fn is_closed(c: &[[f32; 2]]) -> bool {
    c.len() > 2 && c.first() == c.last()
}

/// A stroke of half-width `r` as outlines: a box per segment and a disc per point (round
/// joins and caps), all wound the same way so nonzero filling unions them.
fn stroke(c: &[[f32; 2]], closed: bool, r: f32, out: &mut Vec<Vec<[f32; 2]>>) {
    let n = c.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segments {
        let (a, b) = (c[i], c[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 {
            continue;
        }
        let (nx, ny) = (-dy / len * r, dx / len * r);
        out.push(oriented(vec![
            [a[0] + nx, a[1] + ny],
            [b[0] + nx, b[1] + ny],
            [b[0] - nx, b[1] - ny],
            [a[0] - nx, a[1] - ny],
        ]));
    }
    for p in c {
        out.push(oriented(ellipse(p[0], p[1], r, r)));
    }
}

fn oriented(mut c: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    let n = c.len();
    let area: f32 = (0..n)
        .map(|i| c[i][0] * c[(i + 1) % n][1] - c[(i + 1) % n][0] * c[i][1])
        .sum();
    if area < 0.0 {
        c.reverse();
    }
    c
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<[f32; 2]> {
    (0..32)
        .map(|i| {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            [cx + rx * libm::cosf(a), cy + ry * libm::sinf(a)]
        })
        .collect()
}

fn rect(attrs: &[(String, String)]) -> Result<Vec<Vec<[f32; 2]>>, String> {
    let (x, y) = (
        opt(attrs, "x")?.unwrap_or(0.0),
        opt(attrs, "y")?.unwrap_or(0.0),
    );
    let (w, h) = (num(attrs, "width")?, num(attrs, "height")?);
    let rx = opt(attrs, "rx")?.or(opt(attrs, "ry")?).unwrap_or(0.0);
    let ry = opt(attrs, "ry")?.unwrap_or(rx);
    let (rx, ry) = (rx.min(w * 0.5).max(0.0), ry.min(h * 0.5).max(0.0));
    if rx == 0.0 || ry == 0.0 {
        return Ok(vec![vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]]);
    }
    let mut c = Vec::new();
    for (cx, cy, start) in [
        (x + w - rx, y + ry, -0.25),
        (x + w - rx, y + h - ry, 0.0),
        (x + rx, y + h - ry, 0.25),
        (x + rx, y + ry, 0.5),
    ] {
        for i in 0..=8 {
            let a = (start + i as f32 / 32.0) * std::f32::consts::TAU;
            c.push([cx + rx * libm::cosf(a), cy + ry * libm::sinf(a)]);
        }
    }
    Ok(vec![c])
}

fn view_box(attrs: &[(String, String)]) -> Result<[f32; 4], String> {
    let view = match attr(attrs, "viewBox") {
        Some(v) => {
            let n = numbers(v)?;
            let [x, y, w, h] = n[..] else {
                return Err("viewBox takes four numbers".into());
            };
            [x, y, w, h]
        }
        None => [0.0, 0.0, num(attrs, "width")?, num(attrs, "height")?],
    };
    if !(view[2] > 0.0 && view[3] > 0.0) {
        return Err("the icon's viewBox needs a positive size".into());
    }
    Ok(view)
}

fn paint(attrs: &[(String, String)], parent: Paint) -> Result<Paint, String> {
    let mut p = parent;
    let mut props: Vec<(String, String)> = attrs.to_vec();
    if let Some(style) = attr(attrs, "style") {
        for decl in style.split(';').filter(|d| !d.trim().is_empty()) {
            let (k, v) = decl
                .split_once(':')
                .ok_or_else(|| format!("style `{decl}` is not key:value"))?;
            props.push((k.trim().into(), v.trim().into()));
        }
    }
    for (key, value) in &props {
        match key.as_str() {
            "fill" => p.fill = colour(value)?,
            "stroke" => p.stroke = colour(value)?,
            "stroke-width" => p.stroke_width = number(value)?,
            "fill-rule" => p.even_odd = value == "evenodd",
            "opacity" | "fill-opacity" | "stroke-opacity" => p.opacity *= number(value)?,
            // Effects drawn from `<defs>`: skipping them would draw a different icon.
            "filter" | "clip-path" | "mask" | "marker" | "marker-start" | "marker-mid"
            | "marker-end"
                if value.trim() != "none" =>
            {
                return Err(format!("`{key}` is not supported in icons"));
            }
            _ => {}
        }
    }
    if let Some(t) = attr(attrs, "transform") {
        p.transform = multiply(parent.transform, transform(t)?);
    }
    Ok(p)
}

fn colour(v: &str) -> Result<Option<[u8; 4]>, String> {
    let v = v.trim();
    Ok(Some(match v {
        "none" | "transparent" => return Ok(None),
        "currentColor" | "black" => [0, 0, 0, 255],
        "white" => [255, 255, 255, 255],
        _ => {
            let hex = v
                .strip_prefix('#')
                .ok_or_else(|| format!("colour `{v}`: use #rgb, #rrggbb, currentColor or none"))?;
            let hex = if hex.len() == 3 {
                hex.chars().flat_map(|c| [c, c]).collect()
            } else {
                hex.to_string()
            };
            let n = u32::from_str_radix(&hex, 16)
                .ok()
                .filter(|_| hex.len() == 6)
                .ok_or_else(|| format!("colour `{v}` is not #rgb or #rrggbb"))?;
            [(n >> 16) as u8, (n >> 8) as u8, n as u8, 255]
        }
    }))
}

fn multiply(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

fn transform(text: &str) -> Result<[f32; 6], String> {
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut rest = text.trim();
    while !rest.is_empty() {
        let (name, after) = rest
            .split_once('(')
            .ok_or_else(|| format!("transform `{text}` is not understood"))?;
        let (args, after) = after
            .split_once(')')
            .ok_or_else(|| format!("transform `{text}` is not closed"))?;
        let n = numbers(args)?;
        let get = |i: usize, d: f32| n.get(i).copied().unwrap_or(d);
        let t = match name.trim() {
            "translate" => [1.0, 0.0, 0.0, 1.0, get(0, 0.0), get(1, 0.0)],
            "scale" => [get(0, 1.0), 0.0, 0.0, get(1, get(0, 1.0)), 0.0, 0.0],
            "matrix" if n.len() == 6 => [n[0], n[1], n[2], n[3], n[4], n[5]],
            "rotate" => {
                let a = get(0, 0.0).to_radians();
                let (c, s) = (libm::cosf(a), libm::sinf(a));
                let (cx, cy) = (get(1, 0.0), get(2, 0.0));
                multiply(
                    [1.0, 0.0, 0.0, 1.0, cx, cy],
                    multiply([c, s, -s, c, 0.0, 0.0], [1.0, 0.0, 0.0, 1.0, -cx, -cy]),
                )
            }
            other => return Err(format!("transform `{other}` is not supported")),
        };
        m = multiply(m, t);
        rest = after.trim_start_matches([' ', ',']).trim();
    }
    Ok(m)
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}
fn opt(attrs: &[(String, String)], key: &str) -> Result<Option<f32>, String> {
    attr(attrs, key).map(number).transpose()
}
fn num(attrs: &[(String, String)], key: &str) -> Result<f32, String> {
    opt(attrs, key)?.ok_or_else(|| format!("missing `{key}`"))
}
fn number(v: &str) -> Result<f32, String> {
    let n: f32 = v
        .trim()
        .trim_end_matches("px")
        .parse()
        .map_err(|_| format!("`{v}` is not a number"))?;
    n.is_finite()
        .then_some(n)
        .ok_or_else(|| format!("`{v}` is not finite"))
}
fn numbers(v: &str) -> Result<Vec<f32>, String> {
    Lexer::new(v).rest()
}

/// Path data flattened into contours.
fn path(d: &str) -> Result<Vec<Vec<[f32; 2]>>, String> {
    let mut lx = Lexer::new(d);
    let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut cur = [0.0f32; 2];
    let mut start = cur;
    let mut last_ctrl: Option<[f32; 2]> = None;
    let mut cmd = ' ';
    loop {
        lx.skip();
        let Some(c) = lx.peek() else { break };
        if c.is_ascii_alphabetic() {
            cmd = c;
            lx.pos += 1;
        } else if cmd == ' ' {
            return Err(format!("path data `{d}` must start with a command"));
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur } else { [0.0, 0.0] };
        let pt = |lx: &mut Lexer| -> Result<[f32; 2], String> {
            Ok([lx.num()? + base[0], lx.num()? + base[1]])
        };
        let line = |contours: &mut Vec<Vec<[f32; 2]>>, p: [f32; 2]| {
            if let Some(c) = contours.last_mut() {
                c.push(p);
            }
        };
        let mut ctrl = None;
        match cmd.to_ascii_uppercase() {
            'M' => {
                cur = pt(&mut lx)?;
                start = cur;
                contours.push(vec![cur]);
                // Further pairs after a moveto are linetos.
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' => {
                cur = pt(&mut lx)?;
                line(&mut contours, cur);
            }
            'H' => {
                cur[0] = lx.num()? + if rel { cur[0] } else { 0.0 };
                line(&mut contours, cur);
            }
            'V' => {
                cur[1] = lx.num()? + if rel { cur[1] } else { 0.0 };
                line(&mut contours, cur);
            }
            'Z' => {
                cur = start;
                if let Some(c) = contours.last_mut()
                    && c.last() != Some(&start)
                {
                    c.push(start);
                }
                contours.push(vec![cur]);
                cmd = ' ';
            }
            k @ ('C' | 'S' | 'Q' | 'T') => {
                let (c1, c2, end) = match k {
                    'C' => (pt(&mut lx)?, Some(pt(&mut lx)?), pt(&mut lx)?),
                    'S' => {
                        let c1 = reflect(cur, last_ctrl);
                        (c1, Some(pt(&mut lx)?), pt(&mut lx)?)
                    }
                    'Q' => (pt(&mut lx)?, None, pt(&mut lx)?),
                    _ => (reflect(cur, last_ctrl), None, pt(&mut lx)?),
                };
                for i in 1..=12 {
                    let t = i as f32 / 12.0;
                    let u = 1.0 - t;
                    let p = match c2 {
                        Some(c2) => [0, 1].map(|a| {
                            u * u * u * cur[a]
                                + 3.0 * u * u * t * c1[a]
                                + 3.0 * u * t * t * c2[a]
                                + t * t * t * end[a]
                        }),
                        None => {
                            [0, 1].map(|a| u * u * cur[a] + 2.0 * u * t * c1[a] + t * t * end[a])
                        }
                    };
                    line(&mut contours, p);
                }
                ctrl = Some(c2.unwrap_or(c1));
                cur = end;
            }
            'A' => {
                let (rx, ry, rot) = (lx.num()?.abs(), lx.num()?.abs(), lx.num()?);
                let (large, sweep) = (lx.flag()?, lx.flag()?);
                let end = pt(&mut lx)?;
                for p in arc(cur, end, rx, ry, rot, large, sweep) {
                    line(&mut contours, p);
                }
                cur = end;
            }
            other => return Err(format!("path command `{other}` is not supported")),
        }
        // Smooth curves reflect only a previous curve's control point of the same kind.
        last_ctrl = ctrl;
    }
    contours.retain(|c| c.len() > 1);
    Ok(contours)
}

fn reflect(cur: [f32; 2], ctrl: Option<[f32; 2]>) -> [f32; 2] {
    ctrl.map_or(cur, |c| [2.0 * cur[0] - c[0], 2.0 * cur[1] - c[1]])
}

/// An elliptical arc's points after `from`, by the SVG endpoint-to-centre conversion.
fn arc(
    from: [f32; 2],
    to: [f32; 2],
    mut rx: f32,
    mut ry: f32,
    rot: f32,
    large: bool,
    sweep: bool,
) -> Vec<[f32; 2]> {
    if rx == 0.0 || ry == 0.0 || from == to {
        return vec![to];
    }
    let phi = rot.to_radians();
    let (c, s) = (libm::cosf(phi), libm::sinf(phi));
    let (dx, dy) = ((from[0] - to[0]) * 0.5, (from[1] - to[1]) * 0.5);
    let (x1, y1) = (c * dx + s * dy, -s * dx + c * dy);
    let lambda = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (num / den).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
    let (cx, cy) = (
        c * cx1 - s * cy1 + (from[0] + to[0]) * 0.5,
        s * cx1 + c * cy1 + (from[1] + to[1]) * 0.5,
    );
    let angle = |ux: f32, uy: f32| libm::atan2f(uy, ux);
    let a0 = angle((x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut da = angle((-x1 - cx1) / rx, (-y1 - cy1) / ry) - a0;
    let tau = std::f32::consts::TAU;
    if sweep && da < 0.0 {
        da += tau;
    } else if !sweep && da > 0.0 {
        da -= tau;
    }
    let steps = ((da.abs() / tau * 32.0).ceil() as usize).max(2);
    (1..=steps)
        .map(|i| {
            let a = a0 + da * i as f32 / steps as f32;
            let (x, y) = (rx * libm::cosf(a), ry * libm::sinf(a));
            if i == steps {
                to
            } else {
                [c * x - s * y + cx, s * x + c * y + cy]
            }
        })
        .collect()
}

struct Lexer<'a> {
    s: &'a [u8],
    pos: usize,
}
impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            s: s.as_bytes(),
            pos: 0,
        }
    }
    fn skip(&mut self) {
        while self
            .s
            .get(self.pos)
            .is_some_and(|c| c.is_ascii_whitespace() || *c == b',')
        {
            self.pos += 1;
        }
    }
    fn peek(&self) -> Option<char> {
        self.s.get(self.pos).map(|c| *c as char)
    }
    fn flag(&mut self) -> Result<bool, String> {
        self.skip();
        match self.peek() {
            Some('0') => {
                self.pos += 1;
                Ok(false)
            }
            Some('1') => {
                self.pos += 1;
                Ok(true)
            }
            _ => Err("an arc flag is 0 or 1".into()),
        }
    }
    fn num(&mut self) -> Result<f32, String> {
        self.skip();
        let start = self.pos;
        let mut seen_dot = false;
        let mut seen_e = false;
        while let Some(&c) = self.s.get(self.pos) {
            let at_start = self.pos == start;
            let after_e = self.pos > start && matches!(self.s[self.pos - 1], b'e' | b'E');
            match c {
                b'0'..=b'9' => {}
                b'-' | b'+' if at_start || after_e => {}
                b'.' if !seen_dot && !seen_e => seen_dot = true,
                b'e' | b'E' if !seen_e && !at_start => seen_e = true,
                _ => break,
            }
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.pos]).unwrap_or("");
        if text.is_empty() {
            return Err(format!(
                "expected a number at `{}`",
                String::from_utf8_lossy(&self.s[start..(start + 12).min(self.s.len())])
            ));
        }
        number(text)
    }
    fn rest(&mut self) -> Result<Vec<f32>, String> {
        let mut out = Vec::new();
        loop {
            self.skip();
            if self.peek().is_none() {
                return Ok(out);
            }
            out.push(self.num()?);
        }
    }
}

enum Tag {
    /// Name, attributes, and whether it closes itself.
    Open(String, Vec<(String, String)>, bool),
    Close(String),
}

/// The element tags of an XML document; text, comments and declarations are skipped.
fn tags(text: &str) -> Result<Vec<Tag>, String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        if let Some(r) = rest.strip_prefix("!--") {
            rest = &r[r.find("-->").ok_or("an unclosed comment")? + 3..];
            continue;
        }
        let end = rest.find('>').ok_or("an unclosed tag")?;
        let body = &rest[..end];
        rest = &rest[end + 1..];
        if body.starts_with('?') || body.starts_with('!') {
            continue;
        }
        if let Some(name) = body.strip_prefix('/') {
            out.push(Tag::Close(name.trim().into()));
            continue;
        }
        let empty = body.ends_with('/');
        let body = body.trim_end_matches('/');
        let name_end = body
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(body.len());
        let name = body[..name_end].to_string();
        let mut attrs = Vec::new();
        let mut a = body[name_end..].trim();
        while !a.is_empty() {
            let (key, after) = a
                .split_once('=')
                .ok_or_else(|| format!("<{name}>: attribute `{a}` has no value"))?;
            let after = after.trim_start();
            let quote = after
                .chars()
                .next()
                .filter(|q| *q == '"' || *q == '\'')
                .ok_or_else(|| format!("<{name}>: quote the value of `{}`", key.trim()))?;
            let close = after[1..]
                .find(quote)
                .ok_or_else(|| format!("<{name}>: unclosed value of `{}`", key.trim()))?;
            attrs.push((key.trim().to_string(), after[1..1 + close].to_string()));
            a = after[close + 2..].trim();
        }
        out.push(Tag::Open(name, attrs, empty));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
        <!-- an open book -->
        <path d="M2 4h7a3 3 0 0 1 3 3v13a2 2 0 0 0-2-2H2z"/>
        <path d="M22 4h-7a3 3 0 0 0-3 3v13a2 2 0 0 1 2-2h8z" fill="#ff0000"/>
    </svg>"##;

    #[test]
    fn a_path_icon_rasterises_with_its_holes_and_colours() {
        let icon = Icon::parse(BOOK).unwrap();
        assert_eq!(icon.view, [0.0, 0.0, 24.0, 24.0]);
        assert_eq!(icon.shapes.len(), 2);
        let px = icon.rasterise(48, 48);
        let at = |x: usize, y: usize| &px[(y * 48 + x) * 4..(y * 48 + x) * 4 + 4];
        // Left page black, right page red, the gutter and margins empty.
        assert_eq!(at(10, 20), &[0, 0, 0, 255]);
        assert_eq!(at(38, 20), &[255, 0, 0, 255]);
        assert_eq!(at(24, 2)[3], 0);
        assert_eq!(at(1, 30)[3], 0);
        // Edges are anti-aliased.
        assert!(px.chunks(4).any(|p| p[3] > 0 && p[3] < 255));
    }

    #[test]
    fn shapes_strokes_and_transforms() {
        let icon = Icon::parse(
            r##"<svg width="20" height="20"><g transform="translate(10 10)">
            <circle cx="0" cy="0" r="4"/><rect x="-9" y="-9" width="4" height="4" rx="1"/>
            <line x1="5" y1="-8" x2="5" y2="8" stroke="#00ff00" stroke-width="2"/></g></svg>"##,
        )
        .unwrap();
        let px = icon.rasterise(20, 20);
        let at = |x: usize, y: usize| &px[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4];
        assert_eq!(at(10, 10), &[0, 0, 0, 255]);
        assert_eq!(at(3, 3)[3], 255);
        assert_eq!(at(15, 10), &[0, 255, 0, 255]);
        assert_eq!(at(18, 18)[3], 0);
    }

    #[test]
    fn unsupported_and_broken_icons_are_refused_by_name() {
        for (svg, expected) in [
            ("<svg viewBox='0 0 4 4'><text>hi</text></svg>", "<text>"),
            ("<svg viewBox='0 0 4 4'><path d='M0 0 X 1'/></svg>", "`X`"),
            ("<svg viewBox='0 0 4 4'><g></svg>", "closes nothing"),
            ("<svg viewBox='0 0 4 4'></svg>", "draws nothing"),
            (
                "<svg viewBox='0 0 0 4'><rect width='1' height='1'/></svg>",
                "positive size",
            ),
            ("<path d='M0 0L1 1'/>", "starts with <svg>"),
            (
                "<svg viewBox='0 0 4 4'><rect width='1' height='1' fill='red'/></svg>",
                "#rgb",
            ),
            (
                "<svg viewBox='0 0 4 4'><defs><filter id='b'><feGaussianBlur/></filter></defs>\
                 <path filter='url(#b)' d='M0 0H4V4Z'/></svg>",
                "`filter` is not supported",
            ),
            (
                "<svg viewBox='0 0 4 4'><g style='clip-path: url(#c)'>\
                 <rect width='1' height='1'/></g></svg>",
                "`clip-path` is not supported",
            ),
        ] {
            let e = Icon::parse(svg).unwrap_err();
            assert!(e.contains(expected), "{svg}: {e}");
        }
        assert!(Icon::parse(&" ".repeat(MAX_BYTES + 1)).is_err());
    }

    #[test]
    fn arcs_and_smooth_curves_close_round_shapes() {
        let icon = Icon::parse(
            "<svg viewBox='0 0 10 10'><path d='M1 5a4 4 0 1 0 8 0a4 4 0 1 0-8 0z'/>\
             <path d='M0 0q1 0 1 1t1 1' fill='none' stroke='#000' stroke-width='0.5'/></svg>",
        )
        .unwrap();
        let px = icon.rasterise(10, 10);
        assert_eq!(px[(5 * 10 + 5) * 4 + 3], 255);
        assert_eq!(px[(9 * 10 + 9) * 4 + 3], 0);
    }
}

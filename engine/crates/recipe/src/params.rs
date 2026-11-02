//! A shape's parameters: what each key takes, read from a recipe's TOML with the key named
//! in every refusal.

use std::collections::BTreeMap;

use glam::{Vec2, Vec3};
use toml::Value;

/// What one key takes.
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// A number in `min..=max`; `None` default means the key is required.
    Num(f32, f32, Option<f32>),
    /// A whole number in `min..=max`.
    Int(u32, u32, Option<u32>),
    /// `true` or `false`, with a default.
    Flag(bool),
    /// A list of `[a, b]` pairs, `min..=max` of them, each number within `-limit..=limit`.
    Pairs(usize, usize),
    /// `[x, y, z]`, each in `min..=max`.
    Vec3(f32, f32, Option<[f32; 3]>),
}

/// One key a shape takes.
#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub key: &'static str,
    pub kind: Kind,
    pub about: &'static str,
}

/// Largest coordinate in a list of pairs.
pub const PAIR_LIMIT: f32 = 500.0;

impl Param {
    /// `"1 to 40; default 8: tiers of windows"` for `galaxy-gm fields recipe`.
    pub fn describe(&self) -> String {
        let (range, default) = match self.kind {
            Kind::Num(lo, hi, d) => (
                format!("number {lo} to {hi}"),
                d.map_or("required".into(), |d| format!("default {d}")),
            ),
            Kind::Int(lo, hi, d) => (
                format!("whole number {lo} to {hi}"),
                d.map_or("required".into(), |d| format!("default {d}")),
            ),
            Kind::Flag(d) => ("true or false".into(), format!("default {d}")),
            Kind::Pairs(lo, hi) => (
                format!("list of {lo} to {hi} [a, b] pairs"),
                "required".into(),
            ),
            Kind::Vec3(lo, hi, d) => (
                format!("[x, y, z], each {lo} to {hi}"),
                d.map_or("required".into(), |d| {
                    format!("default [{}, {}, {}]", d[0], d[1], d[2])
                }),
            ),
        };
        format!("{range}; {default}: {}", self.about)
    }
}

/// A value read for a key.
#[derive(Clone, Debug)]
pub enum Val {
    Num(f32),
    Int(u32),
    Flag(bool),
    Pairs(Vec<Vec2>),
    Vec3(Vec3),
}

/// A shape's keys, read and checked.
#[derive(Clone, Debug, Default)]
pub struct Values(BTreeMap<&'static str, Val>);

impl Values {
    pub fn num(&self, key: &str) -> f32 {
        match self.0.get(key) {
            Some(Val::Num(v)) => *v,
            _ => panic!("shape reads `{key}` as a number, which its table doesn't declare"),
        }
    }

    pub fn int(&self, key: &str) -> u32 {
        match self.0.get(key) {
            Some(Val::Int(v)) => *v,
            _ => panic!("shape reads `{key}` as a whole number, which its table doesn't declare"),
        }
    }

    pub fn flag(&self, key: &str) -> bool {
        match self.0.get(key) {
            Some(Val::Flag(v)) => *v,
            _ => panic!("shape reads `{key}` as a flag, which its table doesn't declare"),
        }
    }

    pub fn pairs(&self, key: &str) -> &[Vec2] {
        match self.0.get(key) {
            Some(Val::Pairs(v)) => v,
            _ => panic!("shape reads `{key}` as pairs, which its table doesn't declare"),
        }
    }

    pub fn vec3(&self, key: &str) -> Vec3 {
        match self.0.get(key) {
            Some(Val::Vec3(v)) => *v,
            _ => panic!("shape reads `{key}` as [x, y, z], which its table doesn't declare"),
        }
    }
}

/// A TOML number as `f32` (integers too), if it is one.
pub fn number(v: &Value) -> Option<f32> {
    match v {
        Value::Float(f) => Some(*f as f32),
        Value::Integer(i) => Some(*i as f32),
        _ => None,
    }
}

/// `v` as a number in `lo..=hi`, or why not.
pub fn ranged(v: &Value, lo: f32, hi: f32) -> Result<f32, String> {
    let n = number(v).ok_or_else(|| format!("is {}; expected a number", show(v)))?;
    if !n.is_finite() || n < lo || n > hi {
        return Err(format!("is {}; expected {lo} to {hi}", show(v)));
    }
    Ok(n)
}

/// `v` as `[x, y, z]` with each in `lo..=hi`, or why not.
pub fn triple(v: &Value, lo: f32, hi: f32) -> Result<Vec3, String> {
    let bad = || format!("is {}; expected [x, y, z], each {lo} to {hi}", show(v));
    let Value::Array(items) = v else {
        return Err(bad());
    };
    if items.len() != 3 {
        return Err(bad());
    }
    let mut out = [0.0; 3];
    for (o, item) in out.iter_mut().zip(items) {
        *o = number(item)
            .filter(|n| n.is_finite() && *n >= lo && *n <= hi)
            .ok_or_else(bad)?;
    }
    Ok(Vec3::from(out))
}

/// A short form of a TOML value for messages.
pub fn show(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 40 {
        format!("{}...", s.chars().take(37).collect::<String>())
    } else {
        s
    }
}

/// Reads `params` from `table`, skipping the keys in `common` (placement and paint, read
/// elsewhere). Errors are `(key, message)`.
pub fn read(
    params: &'static [Param],
    table: &toml::Table,
    common: &[&str],
) -> Result<Values, (String, String)> {
    for key in table.keys() {
        if !common.contains(&key.as_str()) && !params.iter().any(|p| p.key == key) {
            let takes: Vec<&str> = params.iter().map(|p| p.key).collect();
            return Err((
                key.clone(),
                format!(
                    "is not a key this shape takes; it takes {}{}",
                    if takes.is_empty() {
                        "no shape keys".to_owned()
                    } else {
                        takes.join(", ")
                    },
                    " (and the part keys: galaxy-gm fields recipe)"
                ),
            ));
        }
    }
    let mut out = Values::default();
    for p in params {
        let v = table.get(p.key);
        let err = |m: String| (p.key.to_owned(), m);
        let val = match (p.kind, v) {
            (Kind::Num(_, _, Some(d)), None) => Val::Num(d),
            (Kind::Int(_, _, Some(d)), None) => Val::Int(d),
            (Kind::Flag(d), None) => Val::Flag(d),
            (Kind::Vec3(_, _, Some(d)), None) => Val::Vec3(Vec3::from(d)),
            (_, None) => return Err(err("is missing; this shape needs it".into())),
            (Kind::Num(lo, hi, _), Some(v)) => Val::Num(ranged(v, lo, hi).map_err(err)?),
            (Kind::Int(lo, hi, _), Some(v)) => {
                let bad = || {
                    err(format!(
                        "is {}; expected a whole number {lo} to {hi}",
                        show(v)
                    ))
                };
                let Value::Integer(i) = v else {
                    return Err(bad());
                };
                if *i < i64::from(lo) || *i > i64::from(hi) {
                    return Err(bad());
                }
                Val::Int(*i as u32)
            }
            (Kind::Flag(_), Some(v)) => Val::Flag(
                v.as_bool()
                    .ok_or_else(|| err(format!("is {}; expected true or false", show(v))))?,
            ),
            (Kind::Vec3(lo, hi, _), Some(v)) => Val::Vec3(triple(v, lo, hi).map_err(err)?),
            (Kind::Pairs(lo, hi), Some(v)) => {
                let bad = || {
                    err(format!(
                        "is {}; expected {lo} to {hi} [a, b] pairs, each number within \
                         -{PAIR_LIMIT} to {PAIR_LIMIT}",
                        show(v)
                    ))
                };
                let Value::Array(items) = v else {
                    return Err(bad());
                };
                if items.len() < lo || items.len() > hi {
                    return Err(bad());
                }
                let mut pairs = Vec::with_capacity(items.len());
                for item in items {
                    let Some([a, b]) = item.as_array().and_then(|a| {
                        let n: Vec<f32> = a.iter().filter_map(number).collect();
                        (a.len() == 2 && n.len() == 2).then(|| [n[0], n[1]])
                    }) else {
                        return Err(bad());
                    };
                    if !(a.is_finite() && b.is_finite())
                        || a.abs() > PAIR_LIMIT
                        || b.abs() > PAIR_LIMIT
                    {
                        return Err(bad());
                    }
                    pairs.push(Vec2::new(a, b));
                }
                Val::Pairs(pairs)
            }
        };
        out.0.insert(p.key, val);
    }
    Ok(out)
}

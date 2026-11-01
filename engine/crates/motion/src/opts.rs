//! Tool options: `--name value` pairs on the command line, or keys in a check spec, read
//! with typed getters. A tool says which names it takes, so a misspelt one is an error
//! rather than silently ignored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Options for one tool run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opts {
    values: BTreeMap<String, String>,
}

impl Opts {
    pub fn new() -> Opts {
        Opts::default()
    }

    /// `--name value` pairs; `--name` alone (or before another `--`) is `true`.
    pub fn parse(args: &[String]) -> Result<Opts, String> {
        let mut values = BTreeMap::new();
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            let Some(name) = arg.strip_prefix("--") else {
                return Err(format!("expected an option such as --along, not `{arg}`"));
            };
            let (name, value) = match name.split_once('=') {
                Some((n, v)) => (n.to_owned(), v.to_owned()),
                None => match args.get(i + 1).filter(|v| !v.starts_with("--")) {
                    Some(v) => {
                        i += 1;
                        (name.to_owned(), v.clone())
                    }
                    None => (name.to_owned(), "true".to_owned()),
                },
            };
            values.insert(name, value);
            i += 1;
        }
        Ok(Opts { values })
    }

    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        self.values.insert(name.to_owned(), value.into());
    }

    /// Fails on any option not in `known`.
    pub fn only(&self, tool: &str, known: &[&str]) -> Result<(), String> {
        for name in self.values.keys() {
            if !known.contains(&name.as_str()) && name != "image" {
                let list: Vec<String> = known.iter().map(|k| format!("--{k}")).collect();
                return Err(format!(
                    "`{tool}` has no option --{name}; it takes {} and --image",
                    list.join(", ")
                ));
            }
        }
        Ok(())
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn flag(&self, name: &str) -> bool {
        matches!(self.text(name), Some("true" | "yes" | "1"))
    }

    pub fn number(&self, name: &str) -> Result<Option<f32>, String> {
        match self.text(name) {
            None => Ok(None),
            Some(v) => v
                .parse::<f32>()
                .ok()
                .filter(|n| n.is_finite())
                .map(Some)
                .ok_or_else(|| format!("--{name} takes a number, not `{v}`")),
        }
    }

    pub fn count(&self, name: &str) -> Result<Option<usize>, String> {
        match self.text(name) {
            None => Ok(None),
            Some(v) => v
                .parse::<usize>()
                .map(Some)
                .map_err(|_| format!("--{name} takes a whole number, not `{v}`")),
        }
    }

    /// A list of numbers, `1.5,2,3`.
    pub fn numbers(&self, name: &str) -> Result<Option<Vec<f32>>, String> {
        match self.text(name) {
            None => Ok(None),
            Some(v) => v
                .split(',')
                .map(|n| {
                    n.trim()
                        .parse::<f32>()
                        .ok()
                        .filter(|n| n.is_finite())
                        .ok_or_else(|| format!("--{name} takes numbers like 1.5,2,3, not `{v}`"))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
        }
    }

    /// Points, `x,y;x,y`.
    pub fn points(&self, name: &str) -> Result<Option<Vec<[f32; 2]>>, String> {
        let Some(v) = self.text(name) else {
            return Ok(None);
        };
        let bad = || format!("--{name} takes points like 0,0;10,4, not `{v}`");
        let mut out = Vec::new();
        for p in v.split(';').filter(|p| !p.trim().is_empty()) {
            let (x, y) = p.split_once(',').ok_or_else(bad)?;
            let x = x.trim().parse::<f32>().map_err(|_| bad())?;
            let y = y.trim().parse::<f32>().map_err(|_| bad())?;
            out.push([x, y]);
        }
        if out.len() < 2 {
            return Err(bad());
        }
        Ok(Some(out))
    }

    /// A box, `x,y,w,h` in pixels.
    pub fn rect(&self, name: &str) -> Result<Option<[f32; 4]>, String> {
        match self.numbers(name)? {
            None => Ok(None),
            Some(v) if v.len() == 4 && v[2] > 0.0 && v[3] > 0.0 => {
                Ok(Some([v[0], v[1], v[2], v[3]]))
            }
            Some(_) => Err(format!("--{name} takes a box in pixels: x,y,w,h")),
        }
    }

    /// Where the tool's picture goes: `--image`, or `<film>/motion/<name>.png`.
    pub fn image_path(&self, film: &Path, name: &str) -> Result<PathBuf, String> {
        let path = match self.text("image") {
            Some(p) => PathBuf::from(p),
            None => film.join("motion").join(format!("{name}.png")),
        };
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("can't make {}: {e}", dir.display()))?;
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn options_parse_values_switches_and_lists() {
        let o = Opts::parse(&args("--along course --onion --at 1.5,2 --size=3")).unwrap();
        assert_eq!(o.text("along"), Some("course"));
        assert!(o.flag("onion"));
        assert_eq!(o.numbers("at").unwrap(), Some(vec![1.5, 2.0]));
        assert_eq!(o.count("size").unwrap(), Some(3));
        assert!(o.only("sheet", &["along", "onion", "at"]).is_err());
        assert!(o.only("sheet", &["along", "onion", "at", "size"]).is_ok());
        assert!(Opts::parse(&args("course")).is_err());
    }

    #[test]
    fn points_need_two() {
        let mut o = Opts::new();
        o.set("along", "0,0;10,4");
        assert_eq!(
            o.points("along").unwrap(),
            Some(vec![[0.0, 0.0], [10.0, 4.0]])
        );
        o.set("along", "0,0");
        assert!(o.points("along").is_err());
    }
}

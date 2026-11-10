//! Explored memory: the grid cells a thing with `engine.explorer` has flown near, kept in
//! `engine.explored { cell, cells }` as canonical text so it is hashed, saved, replayed and
//! edited like any other field. The sim marks cells (`galaxy_sim::explore`), scripts ask
//! `ctx.explored`, and the view paints `ui.map`; all three read the text through here.
//!
//! A cell is `(floor(x / cell), floor(y / cell))` in plain IEEE arithmetic, so the same
//! position gives the same cell on every OS. `cells` lists the explored cells as rows of
//! runs: `row:start+count,start+count;row:...`, rows by ascending `y` cell, runs by
//! ascending `x` cell, every count at least 1 and runs in a row never touching (a run
//! starts past the previous one's end). `"0:-2+5;1:-3+7"` is cells -2..=2 on row 0 and
//! -3..=3 on row 1. Empty text is nothing explored. There is exactly one text for a set
//! of cells, so the same memory always hashes the same.

use std::collections::BTreeMap;

/// The default cell size, world units.
pub const DEFAULT_CELL: f64 = 8.0;
/// The smallest cell a memory may use, world units.
pub const MIN_CELL: f64 = 0.25;
/// The most cells `radius` may span: a bigger radius would mark thousands of cells a
/// tick, which is an authoring mistake (a radius meant in cells, or a tiny cell).
pub const MAX_RADIUS_CELLS: f64 = 64.0;
/// Cell coordinates stay within this, so the text never holds huge numbers.
pub const MAX_CELL_INDEX: i64 = 1 << 31;

/// A set of explored cells: each row (`y` cell) maps to its runs `(start, end)` of `x`
/// cells, `end` exclusive, sorted and never touching.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    rows: BTreeMap<i64, Vec<(i64, i64)>>,
}

/// The cell holding `(x, y)`, or `None` when the point is not finite or too far out.
pub fn cell_of(x: f64, y: f64, cell: f64) -> Option<(i64, i64)> {
    let cx = libm::floor(x / cell);
    let cy = libm::floor(y / cell);
    let range = -(MAX_CELL_INDEX as f64)..(MAX_CELL_INDEX as f64);
    (range.contains(&cx) && range.contains(&cy)).then_some((cx as i64, cy as i64))
}

/// Checks a memory's cell size.
pub fn check_cell(cell: f64) -> Result<(), String> {
    if !cell.is_finite() || cell < MIN_CELL {
        return Err(format!(
            "cell must be a finite size of at least {MIN_CELL} world units, not {cell}"
        ));
    }
    Ok(())
}

/// Checks an explorer's radius against the cell it marks in.
pub fn check_radius(radius: f64, cell: f64) -> Result<(), String> {
    if !radius.is_finite() || radius < 0.0 {
        return Err(format!(
            "radius must be a finite distance of at least 0 world units, not {radius}"
        ));
    }
    if radius / cell > MAX_RADIUS_CELLS {
        return Err(format!(
            "radius {radius} spans more than {MAX_RADIUS_CELLS} cells of {cell} world units; \
             give a smaller radius or a bigger engine.explored cell"
        ));
    }
    Ok(())
}

fn int(s: &str) -> Option<i64> {
    let n: i64 = s.parse().ok()?;
    // Canonical: no sign on positives, no leading zeros, no -0.
    (n.to_string() == s && (-MAX_CELL_INDEX..MAX_CELL_INDEX).contains(&n)).then_some(n)
}

impl Memory {
    /// Reads `cells` text, refusing anything but the one canonical spelling.
    pub fn parse(text: &str) -> Result<Memory, String> {
        let mut rows = BTreeMap::new();
        if text.is_empty() {
            return Ok(Memory { rows });
        }
        let mut last_row = None;
        for row in text.split(';') {
            let bad = |why: &str| format!("cells: row `{row}`: {why} (rows are `y:x+count,...`)");
            let (y, runs) = row.split_once(':').ok_or_else(|| bad("no `:`"))?;
            let y = int(y).ok_or_else(|| bad("the row is not a whole number"))?;
            if last_row.is_some_and(|l| y <= l) {
                return Err(bad("rows must be in ascending order, each once"));
            }
            last_row = Some(y);
            let mut list: Vec<(i64, i64)> = Vec::new();
            for run in runs.split(',') {
                let (start, count) = run
                    .split_once('+')
                    .ok_or_else(|| bad(&format!("run `{run}` is not start+count")))?;
                let start = int(start).ok_or_else(|| bad(&format!("run `{run}`: bad start")))?;
                let count = int(count)
                    .filter(|c| *c >= 1)
                    .ok_or_else(|| bad(&format!("run `{run}`: count is a whole number from 1")))?;
                let end = start + count;
                if end > MAX_CELL_INDEX {
                    return Err(bad(&format!("run `{run}` ends too far out")));
                }
                if list.last().is_some_and(|l| start <= l.1) {
                    return Err(bad(&format!(
                        "run `{run}` overlaps or touches the run before it; join them"
                    )));
                }
                list.push((start, end));
            }
            rows.insert(y, list);
        }
        Ok(Memory { rows })
    }

    /// The canonical text of these cells.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (y, runs) in &self.rows {
            if !out.is_empty() {
                out.push(';');
            }
            out.push_str(&format!("{y}:"));
            for (i, (start, end)) in runs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!("{start}+{}", end - start));
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Whether cell `(cx, cy)` is explored.
    pub fn contains(&self, cx: i64, cy: i64) -> bool {
        self.rows.get(&cy).is_some_and(|runs| {
            let i = runs.partition_point(|r| r.0 <= cx);
            i > 0 && runs[i - 1].1 > cx
        })
    }

    /// Marks cell `(cx, cy)`; true when it was not explored before.
    pub fn insert(&mut self, cx: i64, cy: i64) -> bool {
        let runs = self.rows.entry(cy).or_default();
        let i = runs.partition_point(|r| r.0 <= cx);
        if i > 0 && runs[i - 1].1 > cx {
            return false;
        }
        let left = i > 0 && runs[i - 1].1 == cx;
        let right = i < runs.len() && runs[i].0 == cx + 1;
        match (left, right) {
            (true, true) => {
                runs[i - 1].1 = runs[i].1;
                runs.remove(i);
            }
            (true, false) => runs[i - 1].1 = cx + 1,
            (false, true) => runs[i].0 = cx,
            (false, false) => runs.insert(i, (cx, cx + 1)),
        }
        true
    }

    /// How many cells are explored.
    pub fn count(&self) -> u64 {
        self.rows
            .values()
            .flatten()
            .map(|(s, e)| (e - s) as u64)
            .sum()
    }

    /// The explored runs of row `cy` that overlap `from..to`, clipped to it.
    pub fn runs(&self, cy: i64, from: i64, to: i64) -> impl Iterator<Item = (i64, i64)> + '_ {
        self.rows
            .get(&cy)
            .into_iter()
            .flatten()
            .filter(move |(s, e)| *e > from && *s < to)
            .map(move |(s, e)| ((*s).max(from), (*e).min(to)))
    }
}

/// Marks every cell around `(x, y)` that `radius` reaches: the cell holding the point,
/// and each cell whose centre is within `radius` of it. Returns whether any was new.
pub fn mark(memory: &mut Memory, x: f64, y: f64, radius: f64, cell: f64) -> bool {
    let Some((cx, cy)) = cell_of(x, y, cell) else {
        return false;
    };
    let mut new = memory.insert(cx, cy);
    let reach = libm::ceil(radius / cell) as i64 + 1;
    let r2 = radius * radius;
    for j in cy - reach..=cy + reach {
        let dy = (j as f64 + 0.5) * cell - y;
        for i in cx - reach..=cx + reach {
            let dx = (i as f64 + 0.5) * cell - x;
            if dx * dx + dy * dy <= r2 && i.abs() < MAX_CELL_INDEX && j.abs() < MAX_CELL_INDEX {
                new |= memory.insert(i, j);
            }
        }
    }
    new
}

/// Whether `(x, y)` lies in an explored cell of `cells` (an `engine.explored` text) with
/// cell size `cell`. Malformed text explores nothing.
pub fn explored_at(cells: &str, cell: f64, x: f64, y: f64) -> bool {
    if check_cell(cell).is_err() {
        return false;
    }
    let Some((cx, cy)) = cell_of(x, y, cell) else {
        return false;
    };
    Memory::parse(cells).is_ok_and(|m| m.contains(cx, cy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_canonical_and_round_trips() {
        let mut m = Memory::default();
        for (x, y) in [(0, 0), (2, 0), (1, 0), (-3, 1), (-1, 1), (5, -2)] {
            assert!(m.insert(x, y));
        }
        assert!(!m.insert(1, 0));
        assert_eq!(m.text(), "-2:5+1;0:0+3;1:-3+1,-1+1");
        assert_eq!(Memory::parse(&m.text()).unwrap(), m);
        assert!(m.insert(-2, 1));
        assert_eq!(m.text(), "-2:5+1;0:0+3;1:-3+3");
        assert_eq!(m.count(), 7);
        assert!(m.contains(-1, 1) && !m.contains(0, 1) && !m.contains(3, 0));
        assert_eq!(Memory::parse("").unwrap(), Memory::default());
    }

    #[test]
    fn non_canonical_text_is_refused_with_a_reason() {
        for (text, why) in [
            ("0:0+2,2+1", "touches"),
            ("0:3+1,1+1", "overlaps"),
            ("1:0+1;0:0+1", "ascending"),
            ("0:0+0", "count"),
            ("0:+1+1", "bad start"),
            ("00:0+1", "whole number"),
            ("0:0+1;", "no `:`"),
            ("0 0+1", "no `:`"),
            ("0:0-1", "start+count"),
        ] {
            let e = Memory::parse(text).unwrap_err();
            assert!(e.contains(why), "{text}: {e}");
        }
    }

    #[test]
    fn marking_covers_the_disc_and_the_cell_under_the_point() {
        let mut m = Memory::default();
        assert!(mark(&mut m, 4.0, 4.0, 0.0, 8.0));
        assert_eq!(m.text(), "0:0+1");
        let mut m = Memory::default();
        mark(&mut m, 0.0, 0.0, 16.0, 8.0);
        // Centres within 16 of the origin: rows -2..=1.
        assert_eq!(m.text(), "-2:-1+2;-1:-2+4;0:-2+4;1:-1+2");
        assert!(!mark(&mut m, 0.5, 0.5, 16.0, 8.0));
        assert!(explored_at(&m.text(), 8.0, -9.0, 7.0));
        assert!(!explored_at(&m.text(), 8.0, 20.0, 0.0));
        assert!(cell_of(f64::NAN, 0.0, 8.0).is_none());
        assert!(check_radius(600.0, 8.0).is_err());
        assert!(check_radius(512.0, 8.0).is_ok());
        assert!(check_cell(0.0).is_err());
    }
}

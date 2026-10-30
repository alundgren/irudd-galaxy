//! The step budget, and what native helpers charge to it.
//!
//! The VM interrupt counts one step per loop back-edge and call, but a native helper
//! (a library function written in C or Rust) can do work in proportion to its input
//! inside a single step. Every helper that does more than a bounded amount of work
//! without allocating it (allocation is already capped by the per-call memory limit,
//! with the collector stopped) charges the budget up front, in proportion to that
//! work. Going over the budget ends the call for good, exactly like running out in the
//! VM: `pcall` can't catch it.
//!
//! Units: one step per table slot walked (array and hash part, full or empty: a walk
//! visits every slot) or element moved, compared or shifted, and one per
//! [`BYTES_PER_STEP`] bytes of string or buffer a helper reads. Views over the world or
//! history charge one per [`FACTS_PER_STEP`] facts or [`RECORDS_PER_STEP`] records
//! they scan.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use galaxy_contract::pack::Location;
use mlua::{Function, Lua, MultiValue, Table, Value};

use crate::vm::{caller_location, whole_number};

/// Bytes of string or buffer per step.
pub(crate) const BYTES_PER_STEP: u64 = 64;
/// History facts a query reads per step.
pub(crate) const FACTS_PER_STEP: u64 = 16;
/// World records (rocks, ...) a view scans per step.
pub(crate) const RECORDS_PER_STEP: u64 = 16;

/// The step counter shared by the interrupt and the native helpers.
#[derive(Default)]
pub(crate) struct Budget {
    left: Cell<u64>,
    pub(crate) exhausted: Cell<bool>,
    pub(crate) at: RefCell<Option<Location>>,
}

impl Budget {
    pub(crate) fn reset(&self, steps: u64) {
        self.left.set(steps);
        self.exhausted.set(false);
        *self.at.borrow_mut() = None;
    }

    /// Takes `steps`. Over the budget, the call is over for good.
    pub(crate) fn charge(&self, lua: &Lua, steps: u64) -> mlua::Result<()> {
        if self.exhausted.get() {
            return Err(mlua::Error::runtime("over budget"));
        }
        let left = self.left.get();
        if steps > left {
            self.left.set(0);
            self.exhausted.set(true);
            *self.at.borrow_mut() = caller_location(lua, 1).or_else(|| caller_location(lua, 0));
            return Err(mlua::Error::runtime("over budget"));
        }
        self.left.set(left - steps);
        Ok(())
    }

    /// The interrupt: one step, or the end of the call.
    pub(crate) fn tick(&self, lua: &Lua) -> mlua::Result<()> {
        if self.exhausted.get() {
            return Err(mlua::Error::runtime("over budget"));
        }
        let left = self.left.get();
        if left == 0 {
            self.exhausted.set(true);
            *self.at.borrow_mut() = caller_location(lua, 0).or_else(|| caller_location(lua, 1));
            return Err(mlua::Error::runtime("over budget"));
        }
        self.left.set(left - 1);
        Ok(())
    }
}

/// The slots a walk of `t` visits: its array part plus its hash part, full or empty.
/// A table emptied after it grew keeps its slots, so counting entries would let a
/// script walk a large empty table for one step.
///
/// mlua doesn't expose a table's sizes, so this reads them from Luau's `LuaTable`
/// header (`VM/src/lobject.h` in the pinned `luau0-src`): eight one-byte fields
/// (`tt`, `marked`, `memcat`, `tmcache`, `readonly`, `safeenv`, `lsizenode`,
/// `nodemask8`), then `int sizearray`. The test `table_slots_match_luau` checks the
/// offsets against tables of known size, so a Luau upgrade that moves them fails it.
#[allow(unsafe_code)]
pub(crate) fn table_slots(t: &Table) -> u64 {
    let p = t.to_pointer().cast::<u8>();
    if p.is_null() {
        return 0;
    }
    // SAFETY: `lua_topointer` on a table returns its `LuaTable`, which is alive while
    // we hold `t` and is at least 12 bytes long; we read bytes 6 and 8..12 only.
    let (lsizenode, sizearray) =
        unsafe { (p.add(6).read(), p.add(8).cast::<i32>().read_unaligned()) };
    let node = 1u64 << u32::from(lsizenode).min(40);
    u64::try_from(sizearray).unwrap_or(0).saturating_add(node)
}

/// Steps for reading `bytes` bytes.
pub(crate) fn bytes_cost(bytes: u64) -> u64 {
    1 + bytes / BYTES_PER_STEP
}

fn arg(args: &MultiValue, i: usize) -> Option<&Value> {
    args.get(i)
}

/// A whole-number argument, or `default` if it's absent or not a whole number (the
/// helper itself then reports a bad argument).
fn int_arg(args: &MultiValue, i: usize, default: i64) -> i64 {
    match arg(args, i) {
        Some(v @ (Value::Integer(_) | Value::Number(_))) => match whole_number(v) {
            Some(n) => n,
            // Fractional or huge: as big as it can get, so it's charged in full.
            None => match v {
                Value::Number(n) if *n < 0.0 => i64::MIN,
                _ => i64::MAX,
            },
        },
        _ => default,
    }
}

fn len_arg(args: &MultiValue, i: usize) -> u64 {
    match arg(args, i) {
        Some(Value::String(s)) => s.as_bytes().len() as u64,
        Some(Value::Buffer(b)) => b.len() as u64,
        Some(Value::Table(t)) => t.raw_len() as u64,
        // Numbers are converted to (short) strings.
        _ => 0,
    }
}

fn slots_arg(args: &MultiValue, i: usize) -> u64 {
    match arg(args, i) {
        Some(Value::Table(t)) => table_slots(t),
        _ => 0,
    }
}

/// Elements from `first` to `last` (inclusive), 0 if empty, saturating.
fn span(first: i64, last: i64) -> u64 {
    u64::try_from(i128::from(last) - i128::from(first) + 1).unwrap_or(0)
}

/// Replaces `lib[name]` with a function that charges `cost(args)` and then calls the
/// original.
fn metered(
    lua: &Lua,
    lib: &Table,
    name: &str,
    budget: &Rc<Budget>,
    cost: impl Fn(&MultiValue) -> u64 + 'static,
) -> mlua::Result<()> {
    let Ok(Value::Function(raw)) = lib.raw_get::<Value>(name) else {
        return Ok(());
    };
    let budget = budget.clone();
    let f = lua.create_function(move |lua, args: MultiValue| {
        budget.charge(lua, cost(&args))?;
        raw.call::<MultiValue>(args)
    })?;
    lib.raw_set(name, f)
}

/// Charges the native library functions that do work in proportion to their input.
/// Runs before the sandbox freezes the libraries.
pub(crate) fn install(lua: &Lua, globals: &Table, budget: &Rc<Budget>) -> mlua::Result<()> {
    let table: Table = globals.get("table")?;
    // Removed like `table.foreach`: deprecated, and they walk a table natively.
    table.raw_set("foreachi", Value::Nil)?;
    metered(lua, &table, "sort", budget, |a| {
        let n = len_arg(a, 0);
        let depth = u64::from(64 - n.leading_zeros());
        1 + n.saturating_mul(depth.max(1))
    })?;
    metered(lua, &table, "find", budget, |a| 1 + len_arg(a, 0))?;
    metered(lua, &table, "insert", budget, |a| {
        if a.len() >= 3 { 1 + len_arg(a, 0) } else { 1 }
    })?;
    metered(lua, &table, "remove", budget, |a| {
        if a.len() >= 2 { 1 + len_arg(a, 0) } else { 1 }
    })?;
    metered(lua, &table, "move", budget, |a| {
        1 + span(int_arg(a, 1, 1), int_arg(a, 2, 0))
    })?;
    let ranged = |a: &MultiValue, first: usize| {
        let n = i64::try_from(len_arg(a, 0)).unwrap_or(i64::MAX);
        1 + span(int_arg(a, first, 1), int_arg(a, first + 1, n))
    };
    metered(lua, &table, "concat", budget, move |a| ranged(a, 2))?;
    metered(lua, &table, "unpack", budget, move |a| ranged(a, 1))?;
    metered(lua, globals, "unpack", budget, move |a| ranged(a, 1))?;
    metered(lua, &table, "create", budget, |a| {
        1 + u64::try_from(int_arg(a, 0, 0)).unwrap_or(0)
    })?;
    for name in ["clone", "clear", "maxn"] {
        metered(lua, &table, name, budget, |a| 1 + slots_arg(a, 0))?;
    }

    let string: Table = globals.get("string")?;
    metered(lua, &string, "rep", budget, |a| {
        let n = u64::try_from(int_arg(a, 1, 0)).unwrap_or(0);
        // An empty string still loops `n` times.
        bytes_cost(len_arg(a, 0).max(1).saturating_mul(n))
    })?;
    metered(lua, &string, "sub", budget, |a| {
        let l = len_arg(a, 0);
        let n = i64::try_from(l).unwrap_or(i64::MAX);
        // Luau's `posrelat`: negative positions count from the end.
        let at = |p: i64| {
            if p < 0 {
                n.saturating_add(p).saturating_add(1)
            } else {
                p
            }
        };
        let first = at(int_arg(a, 1, 1)).max(1);
        let last = at(int_arg(a, 2, -1)).min(n);
        bytes_cost(span(first, last))
    })?;
    // From `init` (argument `i`) to the end of the subject.
    let rest = |a: &MultiValue, i: usize| {
        let l = len_arg(a, 0);
        let n = i64::try_from(l).unwrap_or(i64::MAX);
        let init = int_arg(a, i, 1);
        let from = if init < 0 {
            n.saturating_add(init).saturating_add(1)
        } else {
            init
        };
        span(from.max(1), n)
    };
    // A plain `find` (or one whose pattern has no special characters) may compare the
    // pattern at every position.
    metered(lua, &string, "find", budget, move |a| {
        bytes_cost(rest(a, 2).saturating_mul(1 + len_arg(a, 1) / 4))
    })?;
    // Pattern matching checks the interrupt as it recurses; this covers the scans
    // between those checks and hashing the captures.
    metered(lua, &string, "match", budget, move |a| {
        bytes_cost(rest(a, 2))
    })?;
    for name in ["gmatch", "gsub", "split"] {
        metered(lua, &string, name, budget, |a| bytes_cost(len_arg(a, 0)))?;
    }
    metered(lua, &string, "unpack", budget, |a| {
        bytes_cost(len_arg(a, 1))
    })?;

    let utf8: Table = globals.get("utf8")?;
    for name in ["len", "offset", "codepoint"] {
        metered(lua, &utf8, name, budget, |a| bytes_cost(len_arg(a, 0)))?;
    }

    let buffer: Table = globals.get("buffer")?;
    let count = |a: &MultiValue, i: usize, or: u64| match arg(a, i) {
        Some(Value::Nil) | None => or,
        Some(_) => u64::try_from(int_arg(a, i, 0)).unwrap_or(0),
    };
    metered(lua, &buffer, "create", budget, move |a| {
        bytes_cost(count(a, 0, 0))
    })?;
    metered(lua, &buffer, "fromstring", budget, |a| {
        bytes_cost(len_arg(a, 0))
    })?;
    metered(lua, &buffer, "tostring", budget, |a| {
        bytes_cost(len_arg(a, 0))
    })?;
    metered(lua, &buffer, "readstring", budget, move |a| {
        bytes_cost(count(a, 2, 0))
    })?;
    metered(lua, &buffer, "writestring", budget, move |a| {
        bytes_cost(count(a, 3, len_arg(a, 2)))
    })?;
    metered(lua, &buffer, "fill", budget, move |a| {
        bytes_cost(count(a, 3, len_arg(a, 0)))
    })?;
    metered(lua, &buffer, "copy", budget, move |a| {
        bytes_cost(count(a, 4, len_arg(a, 2)))
    })?;
    Ok(())
}

/// The sandbox's `pairs` and `next`, which refuse tables keyed by tables or functions
/// (their order follows heap addresses) and charge for walking the table.
///
/// - `next(t, nil)` and `pairs(t)` walk the whole table first: `1 +` its slots.
/// - `pairs(t)` returns its own iterator. Continuing its traversal (called with the
///   key it returned last) costs nothing more: the walk was paid for. Any other call
///   costs like `next(t, k)`.
/// - `next(t, k)` scans from `k` to the next entry, which can cross empty slots: it
///   costs `1 +` the table's slots / 64, so a traversal with `next` costs at most a
///   few walks' worth, and calling it over and over with the same key is charged.
pub(crate) fn install_iteration(
    lua: &Lua,
    globals: &Table,
    budget: &Rc<Budget>,
) -> mlua::Result<()> {
    let raw_next: Function = globals.get("next")?;

    let b = budget.clone();
    let raw = raw_next.clone();
    let next = lua.create_function(move |lua, (t, k): (Table, Value)| {
        if k.is_nil() {
            b.charge(lua, 1 + table_slots(&t))?;
            stable_keys(&t, "next")?;
        } else {
            b.charge(lua, 1 + table_slots(&t) / 64)?;
        }
        checked_next(&raw, t, k, "next")
    })?;

    let b = budget.clone();
    let pairs = lua.create_function(move |lua, t: Table| {
        b.charge(lua, 1 + table_slots(&t))?;
        stable_keys(&t, "pairs")?;
        let raw = raw_next.clone();
        let budget = b.clone();
        let table = t.to_pointer();
        // The control value the traversal expects next: nil, then each returned key.
        let expect = RefCell::new(Value::Nil);
        let iter = lua.create_function(move |lua, (s, k): (Table, Value)| {
            let continuing = s.to_pointer() == table && same_key(&k, &expect.borrow());
            if !continuing {
                budget.charge(lua, 1 + table_slots(&s) / 64)?;
            }
            let out = checked_next(&raw, s, k, "pairs")?;
            *expect.borrow_mut() = out.front().cloned().unwrap_or(Value::Nil);
            Ok(out)
        })?;
        Ok((iter, t, Value::Nil))
    })?;
    globals.raw_set("next", next)?;
    globals.raw_set("pairs", pairs)?;
    Ok(())
}

/// The raw `next`, refusing a key (given or returned) whose place in the table
/// depends on its address.
fn checked_next(raw: &Function, t: Table, k: Value, what: &str) -> mlua::Result<MultiValue> {
    if crate::vm::by_reference(&k) {
        return Err(unstable_order(what, &k));
    }
    let out: MultiValue = raw.call((t, k))?;
    if let Some(k) = out.front().filter(|k| crate::vm::by_reference(k)) {
        return Err(unstable_order(what, k));
    }
    Ok(out)
}

/// The same table key: strings by identity (Luau interns every string), numbers by
/// value. Never compares string contents, so it costs the same for any key.
fn same_key(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::String(x), Value::String(y)) => x.to_pointer() == y.to_pointer(),
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::Integer(_) | Value::Number(_), Value::Integer(_) | Value::Number(_)) => {
            as_f64(a) == as_f64(b)
        }
        (Value::Vector(x), Value::Vector(y)) => x == y,
        _ => false,
    }
}

fn as_f64(v: &Value) -> f64 {
    match v {
        Value::Integer(i) => *i as f64,
        Value::Number(n) => *n,
        _ => f64::NAN,
    }
}

/// Fails if a table has a key whose position depends on its address. The caller has
/// charged for the walk.
pub(crate) fn stable_keys(t: &Table, what: &str) -> mlua::Result<()> {
    for pair in t.pairs::<Value, Value>() {
        let (k, _) = pair?;
        if crate::vm::by_reference(&k) {
            return Err(unstable_order(what, &k));
        }
    }
    Ok(())
}

fn unstable_order(what: &str, k: &Value) -> mlua::Error {
    mlua::Error::runtime(format!(
        "{what}: this table has a {} as a key, and the order of such keys changes from \
         run to run; key it by strings or numbers",
        k.type_name()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_slots_match_luau() {
        let lua = Lua::new();
        let slots = |src: &str| table_slots(&lua.load(src).eval::<Table>().unwrap());
        // An empty table has no array part and Luau's one-slot dummy node.
        assert_eq!(slots("return {}"), 1);
        assert_eq!(slots("return table.create(1000)"), 1001);
        assert_eq!(slots("return table.create(37, true)"), 38);
        // Five string keys need eight hash slots.
        assert_eq!(slots("return { a = 1, b = 2, c = 3, d = 4, e = 5 }"), 8);
        // Emptying a table keeps its slots.
        assert_eq!(
            slots("local t = table.create(500, 1) for i = 1, 500 do t[i] = nil end return t"),
            501
        );
    }

    #[test]
    fn keys_compare_without_reading_strings() {
        let lua = Lua::new();
        let a = Value::String(lua.create_string("key").unwrap());
        let b = Value::String(lua.create_string("key").unwrap());
        assert!(same_key(&a, &b));
        assert!(same_key(&Value::Integer(3), &Value::Number(3.0)));
        assert!(!same_key(&Value::Nil, &Value::Integer(0)));
    }
}

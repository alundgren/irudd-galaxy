//! A check, on what the Luau compiler produced, that every generic `for` loop takes its
//! iterator from the guard ([`crate::iteration`]).
//!
//! The source rewrite finds loops with a hand-written scanner. This check doesn't trust it:
//! it reads the compiled bytecode, finds every instruction that starts a generic `for`
//! loop (`FORGPREP` and its `pairs`/`ipairs` variants) and confirms the instruction right
//! before it is a call, with three results into the loop's registers, of a function loaded
//! from the global [`crate::iteration::ITERATE`]. The global can only be the engine's
//! function: the globals are frozen and every chunk's environment is read-only, so a script
//! can't replace it, and a local of that name would compile to a different load.
//!
//! The reader follows Luau's own loader (`lvmload.cpp`) for the bytecode versions it
//! accepts. Anything it doesn't understand (another version, an unknown opcode or
//! constant, bytes left over) is an error, so a Luau upgrade that changes the format makes
//! files fail to load instead of skipping the check.

/// Oldest and newest bytecode versions the reader understands.
const VERSIONS: std::ops::RangeInclusive<u8> = 3..=14;
/// Newest type-information version the reader understands.
const TYPES_MAX: u8 = 3;

// Opcodes, numbered as in Luau's `Bytecode.h` (`enum LuauOpcode`).
const NOP: u8 = 0;
const BREAK: u8 = 1;
const GETGLOBAL: u8 = 7;
const SETGLOBAL: u8 = 8;
const SETUPVAL: u8 = 10;
const CLOSEUPVALS: u8 = 11;
const GETIMPORT: u8 = 12;
const SETTABLE: u8 = 14;
const GETTABLEKS: u8 = 15;
const SETTABLEKS: u8 = 16;
const SETTABLEN: u8 = 18;
const NAMECALL: u8 = 20;
const CALL: u8 = 21;
const RETURN: u8 = 22;
const JUMP: u8 = 23;
const JUMPBACK: u8 = 24;
const JUMPIF: u8 = 25;
const JUMPIFNOT: u8 = 26;
const JUMPIFEQ: u8 = 27;
const JUMPIFNOTLT: u8 = 32;
const NEWTABLE: u8 = 53;
const SETLIST: u8 = 55;
const FORNPREP: u8 = 56;
const FORNLOOP: u8 = 57;
const FORGLOOP: u8 = 58;
const FORGPREP_INEXT: u8 = 59;
const FASTCALL3: u8 = 60;
const FORGPREP_NEXT: u8 = 61;
const NATIVECALL: u8 = 62;
const GETVARARGS: u8 = 63;
const PREPVARARGS: u8 = 65;
const LOADKX: u8 = 66;
const JUMPX: u8 = 67;
const FASTCALL: u8 = 68;
const COVERAGE: u8 = 69;
const CAPTURE: u8 = 70;
const FASTCALL1: u8 = 73;
const FASTCALL2: u8 = 74;
const FASTCALL2K: u8 = 75;
const FORGPREP: u8 = 76;
const JUMPXEQKNIL: u8 = 77;
const JUMPXEQKS: u8 = 80;
const GETUDATAKS: u8 = 83;
const SETUDATAKS: u8 = 84;
const NAMECALLUDATA: u8 = 85;
const NEWCLASSMEMBER: u8 = 86;
const CALLFB: u8 = 87;
const CMPPROTO: u8 = 88;
const FASTPCALL: u8 = 89;
const NEWCLASS: u8 = 90;
/// One past the last opcode.
const OPCODES: u8 = 91;

/// Words an instruction takes: 2 for the opcodes followed by an AUX word (Luau's
/// `getOpLength`).
fn length(op: u8) -> usize {
    match op {
        GETGLOBAL
        | SETGLOBAL
        | GETIMPORT
        | GETTABLEKS
        | SETTABLEKS
        | NAMECALL
        | JUMPIFEQ..=JUMPIFNOTLT
        | NEWTABLE
        | SETLIST
        | FORGLOOP
        | LOADKX
        | FASTCALL2
        | FASTCALL2K
        | FASTCALL3
        | JUMPXEQKNIL..=JUMPXEQKS
        | GETUDATAKS
        | SETUDATAKS
        | NAMECALLUDATA
        | NEWCLASSMEMBER
        | CALLFB
        | CMPPROTO
        | NEWCLASS => 2,
        _ => 1,
    }
}

/// Whether an instruction may write register `r`. Errs on the side of yes.
fn may_write(op: u8, a: u8, r: u8) -> bool {
    match op {
        // No register written (the A field, if any, is a source, a count or an id).
        NOP
        | BREAK
        | SETGLOBAL
        | SETUPVAL
        | CLOSEUPVALS
        | SETTABLE
        | SETTABLEKS
        | SETTABLEN
        | SETUDATAKS
        | SETLIST
        | RETURN
        | JUMP
        | JUMPBACK
        | JUMPIF
        | JUMPIFNOT
        | JUMPIFEQ..=JUMPIFNOTLT
        | JUMPXEQKNIL..=JUMPXEQKS
        | JUMPX
        | FASTCALL
        | FASTCALL1
        | FASTCALL2
        | FASTCALL2K
        | FASTCALL3
        | FASTPCALL
        | COVERAGE
        | CAPTURE
        | PREPVARARGS
        | NATIVECALL
        | CMPPROTO => false,
        // Registers from A upwards.
        CALL | CALLFB | GETVARARGS | NAMECALL | NAMECALLUDATA | FORNPREP | FORNLOOP | FORGLOOP
        | FORGPREP | FORGPREP_NEXT | FORGPREP_INEXT => a <= r,
        _ => a == r,
    }
}

/// A generic `for` loop that doesn't go through the guard, at its line if the bytecode
/// has line information.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Unguarded {
    pub(crate) line: Option<u32>,
}

/// The generic `for` loops in `bytecode` whose iterator doesn't come from a call of the
/// global `guard`. Fails if the bytecode can't be read.
pub(crate) fn unguarded(bytecode: &[u8], guard: &str) -> Result<Vec<Unguarded>, String> {
    let mut r = Reader { b: bytecode, at: 0 };
    let version = r.byte()?;
    if !VERSIONS.contains(&version) {
        return Err(format!("bytecode version {version}"));
    }
    let types = if version >= 4 { r.byte()? } else { 0 };
    if types > TYPES_MAX {
        return Err(format!("type information version {types}"));
    }
    let count = r.count()?;
    let mut strings = Vec::with_capacity(count.min(r.left()));
    for _ in 0..count {
        let len = r.count()?;
        strings.push(r.bytes(len)?);
    }
    if types == 3 {
        while r.byte()? != 0 {
            r.varint()?;
        }
    }
    let mut found = Vec::new();
    for _ in 0..r.count()? {
        let size = if version >= 12 {
            Some(r.count()?)
        } else {
            None
        };
        let start = r.at;
        r.bytes(4)?; // stack size, parameters, upvalues, vararg
        if version >= 4 {
            r.byte()?; // flags
            let typesize = r.count()?;
            r.bytes(typesize)?;
        }
        let words = r.count()?;
        let code = (0..words).map(|_| r.u32()).collect::<Result<Vec<_>, _>>()?;
        let constants = r.count()?;
        let mut k: Vec<Option<&[u8]>> = Vec::with_capacity(constants.min(r.left()));
        for _ in 0..constants {
            k.push(r.constant(&strings)?);
        }
        for _ in 0..r.count()? {
            r.varint()?; // child protos
        }
        r.varint()?; // line defined
        r.varint()?; // debug name
        let lines = if r.byte()? != 0 {
            let gap = u32::from(r.byte()?);
            let intervals = if words == 0 {
                0
            } else {
                ((words - 1) >> gap.min(31)) + 1
            };
            let mut offsets = Vec::with_capacity(words);
            let mut last = 0u8;
            for _ in 0..words {
                last = last.wrapping_add(r.byte()?);
                offsets.push(last);
            }
            let mut base = Vec::with_capacity(intervals.min(r.left()));
            let mut line = 0i32;
            for _ in 0..intervals {
                line = line.wrapping_add(r.u32()? as i32);
                base.push(line);
            }
            Some((gap, offsets, base))
        } else {
            None
        };
        match size {
            Some(size) => {
                r.at = start
                    .checked_add(size)
                    .filter(|&end| end <= bytecode.len() && end >= r.at)
                    .ok_or("a function's size runs past its data")?;
            }
            None => {
                if r.byte()? != 0 {
                    for _ in 0..r.count()? {
                        r.varint()?;
                        r.varint()?;
                        r.varint()?;
                        r.byte()?;
                    }
                    for _ in 0..r.count()? {
                        r.varint()?;
                    }
                }
                if version >= 11 {
                    for _ in 0..r.count()? {
                        r.byte()?;
                        r.varint()?;
                    }
                }
            }
        }
        let line_at = |pc: usize| {
            let (gap, offsets, base) = lines.as_ref()?;
            let line = base.get(pc >> gap)? + i32::from(*offsets.get(pc)?);
            u32::try_from(line).ok()
        };
        for pc in loops_without_guard(&code, &k, guard.as_bytes())? {
            found.push(Unguarded { line: line_at(pc) });
        }
    }
    r.varint()?; // main function
    if r.left() != 0 {
        return Err("bytes left over after the last function".into());
    }
    Ok(found)
}

/// Where each generic `for` loop in one function's `code` starts without the guard.
fn loops_without_guard(
    code: &[u32],
    k: &[Option<&[u8]>],
    guard: &[u8],
) -> Result<Vec<usize>, String> {
    let mut starts = Vec::new();
    let mut pc = 0;
    while pc < code.len() {
        let op = (code[pc] & 0xff) as u8;
        if op >= OPCODES {
            return Err(format!("unknown opcode {op}"));
        }
        starts.push(pc);
        pc += length(op);
    }
    if pc != code.len() {
        return Err("an instruction runs past the end of its function".into());
    }
    let field = |word: u32, shift: u32| ((word >> shift) & 0xff) as u8;
    let string = |index: u32| k.get(index as usize).copied().flatten();
    // Whether the function in register `r` at the call starting `starts[call]` is loaded
    // from the guard global: the last instruction before it that may write `r` loads it.
    let from_guard = |call: usize, r: u8| {
        for &at in starts[..call].iter().rev() {
            let word = code[at];
            let (op, a) = (field(word, 0), field(word, 8));
            let aux = code.get(at + 1).copied().unwrap_or(0);
            match op {
                GETIMPORT if a == r => {
                    // A path of one name: the top two bits count the names.
                    return aux >> 30 == 1 && string((aux >> 20) & 0x3ff) == Some(guard);
                }
                GETGLOBAL if a == r => return string(aux) == Some(guard),
                _ if may_write(op, a, r) => return false,
                _ => {}
            }
        }
        false
    };
    let mut unguarded = Vec::new();
    for (i, &at) in starts.iter().enumerate() {
        let word = code[at];
        if !matches!(field(word, 0), FORGPREP | FORGPREP_NEXT | FORGPREP_INEXT) {
            continue;
        }
        let base = field(word, 8);
        let guarded = i > 0 && {
            let call = code[starts[i - 1]];
            matches!(field(call, 0), CALL | CALLFB)
                && field(call, 8) == base
                && field(call, 24) == 4 // three results
                && from_guard(i - 1, base)
        };
        if !guarded {
            unguarded.push(at);
        }
    }
    Ok(unguarded)
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn left(&self) -> usize {
        self.b.len() - self.at
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.left() {
            return Err("the bytecode ends early".into());
        }
        let out = &self.b[self.at..self.at + n];
        self.at += n;
        Ok(out)
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            value |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("a number is too long".into())
    }

    fn count(&mut self) -> Result<usize, String> {
        usize::try_from(self.varint()?).map_err(|_| "a count is too large".to_owned())
    }

    /// One constant: the text of a string constant, `None` for any other kind.
    fn constant(&mut self, strings: &[&'a [u8]]) -> Result<Option<&'a [u8]>, String> {
        match self.byte()? {
            0 => {}                  // nil
            1 => _ = self.bytes(1)?, // boolean
            2 => _ = self.bytes(8)?, // number
            3 => {
                let id = self.count()?;
                return match id {
                    0 => Ok(None),
                    _ => strings
                        .get(id - 1)
                        .map(|s| Some(*s))
                        .ok_or_else(|| "a string constant points nowhere".to_owned()),
                };
            }
            4 => _ = self.u32()?, // import
            5 => {
                for _ in 0..self.count()? {
                    self.varint()?;
                }
            }
            6 => _ = self.varint()?,  // closure
            7 => _ = self.bytes(16)?, // vector
            8 => {
                // table with constants
                for _ in 0..self.count()? {
                    self.varint()?;
                    self.u32()?;
                }
            }
            9 => {
                // integer
                self.byte()?;
                self.varint()?;
            }
            10 => {
                // class shape
                self.varint()?;
                let members = self.count()?.saturating_add(self.count()?);
                for _ in 0..members {
                    self.varint()?;
                }
            }
            11 => _ = self.bytes(32)?, // vector of doubles
            other => return Err(format!("unknown constant kind {other}")),
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(src: &str) -> Vec<Option<u32>> {
        let code = mlua::chunk::Compiler::new().compile(src).unwrap();
        unguarded(&code, "guard")
            .unwrap()
            .into_iter()
            .map(|u| u.line)
            .collect()
    }

    #[test]
    fn finds_loops_that_skip_the_guard() {
        assert_eq!(lines("local t = {}\nfor k in t do end"), [Some(2)]);
        assert_eq!(
            lines("for k in pairs(t) do end\nfor i in ipairs(t) do end\nfor k in next, t do end"),
            [Some(1), Some(2), Some(3)]
        );
        // In nested functions too, and a guard called but not used by the loop.
        assert_eq!(
            lines(
                "local function f(t)\n\treturn function()\n\t\tguard(t)\n\t\tfor k in t do end\n\tend\nend"
            ),
            [Some(4)]
        );
        // A local named like the guard isn't the guard.
        assert_eq!(
            lines("local guard = pairs\nfor k in guard(t) do end"),
            [Some(2)]
        );
        // The guard's result must be the loop's iterator, not an argument to it.
        assert_eq!(lines("for k in f(guard(t)) do end"), [Some(1)]);
        assert_eq!(lines("for k in guard(t), 1 do end"), [Some(1)]);
    }

    #[test]
    fn accepts_loops_through_the_guard() {
        for src in [
            "for k, v in guard(t) do end",
            "for k in guard(pairs(t)) do end",
            "local function f(...) for k in guard(...) do end end",
            "for k in guard(next, t, nil) do for v in guard(k) do end end",
            "for k in guard(if c then a else b) do end",
            "for k in guard(function() for j in guard(t) do end end) do end",
            "for i = 1, 3 do end",
            // `getfenv` turns imports into global lookups.
            "local e = getfenv\nfor k in guard(t) do end",
            "--!optimize 2\nlocal function g(t) for k in guard(t) do end end\ng({})\ng({})",
        ] {
            assert_eq!(lines(src), [], "{src}");
        }
    }

    #[test]
    fn refuses_what_it_cant_read() {
        let code = mlua::chunk::Compiler::new().compile("return 1").unwrap();
        assert!(unguarded(&code[..code.len() - 1], "guard").is_err());
        let mut longer = code.clone();
        longer.push(0);
        assert!(unguarded(&longer, "guard").is_err());
        let mut newer = code;
        newer[0] = 15;
        assert!(unguarded(&newer, "guard").is_err());
    }
}

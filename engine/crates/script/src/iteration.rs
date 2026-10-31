//! Generalized iteration (`for k, v in t do`) through the checked traversal.
//!
//! Luau walks a table given straight to `for ... in` natively, in slot order, without
//! calling `pairs` or `next`. For a table keyed by tables or functions that order follows
//! heap addresses, so it changes from run to run. Before a module file is compiled, every
//! generic `for` gets its iterator expressions wrapped in a call to [`ITERATE`]:
//! `for k, v in t do` compiles as `for k, v in __galaxy_iterate(t) do`. The wrapper
//! ([`crate::meter::install_iteration`]) hands a plain table to the same checked traversal
//! as `pairs`, in the same order Luau's own walk uses, and passes everything else
//! (iterator functions, tables with `__iter` or `__call`) through unchanged. Insertions stay
//! on their line, so error lines don't move.
//!
//! The rewrite works on tokens: it skips strings and comments by Luau's lexical rules, finds
//! each `for`, the `in` of a generic loop (a numeric loop has `=` first) and the loop's
//! `do`, which is the first `do` outside brackets and outside any function literal in the
//! iterator expressions. A misplaced insertion can't compile, so a loop it can't place is
//! reported at its line ([`culprit`]) instead of running unchecked.
//!
//! The scanner isn't trusted on its own. After compiling, [`confirm`] reads the bytecode
//! ([`crate::bytecode`]) and refuses the file if any generic loop the compiler produced
//! doesn't take its iterator from [`ITERATE`], so a loop the scanner missed fails to load
//! at its line instead of running in address order.

/// The global the wrapped loops call. Scripts can't use the name.
pub(crate) const ITERATE: &str = "__galaxy_iterate";

/// A loop the rewrite can't check, or a script using [`ITERATE`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Problem {
    /// `None` when the compiled code has no line for it.
    pub(crate) line: Option<u32>,
    pub(crate) message: String,
}

/// A generic `for` loop: where its iterator expressions start and end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Loop {
    line: u32,
    /// Just after `in`.
    open: usize,
    /// At `do`.
    close: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok<'a> {
    Word(&'a str),
    Punct(u8),
    /// An operator ending in `=` (`==`, `~=`, `+=` ...).
    Op,
}

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
    tok: Tok<'a>,
    start: usize,
    end: usize,
    line: u32,
}

/// The tokens of `src` that matter here: words, brackets and `=`. Strings and comments are
/// skipped; the expressions inside an interpolated string's braces are tokens too.
fn tokens(src: &str) -> Vec<Token<'_>> {
    let b = src.as_bytes();
    let n = b.len();
    let mut out = Vec::new();
    let mut line = 1u32;
    // Open interpolated strings: the brace depth inside each one's current `{ ... }`.
    let mut interp: Vec<u32> = Vec::new();
    let mut i = 0;
    let lines = |from: usize, to: usize| {
        u32::try_from(b[from..to.min(n)].iter().filter(|&&c| c == b'\n').count())
            .unwrap_or(u32::MAX)
    };
    while i < n {
        let c = b[i];
        let start = i;
        match c {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b' ' | b'\t' | b'\r' | 0x0b | 0x0c => i += 1,
            b'-' if b.get(i + 1) == Some(&b'-') => {
                i += 2;
                i = match long_bracket(b, i) {
                    Some(level) => skip_long(b, i, level),
                    None => b[i..]
                        .iter()
                        .position(|&c| matches!(c, b'\n' | b'\r' | 0))
                        .map_or(n, |p| i + p),
                };
                line += lines(start, i);
            }
            b'[' if long_bracket(b, i).is_some() => {
                let level = long_bracket(b, i).unwrap_or(0);
                i = skip_long(b, i, level);
                line += lines(start, i);
            }
            b'\'' | b'"' => {
                // Luau's `readQuotedString`: an unescaped line break ends it (as an error).
                i += 1;
                while i < n && b[i] != c && !matches!(b[i], b'\n' | b'\r' | 0) {
                    i = if b[i] == b'\\' {
                        skip_escape(b, i)
                    } else {
                        i + 1
                    };
                }
                if i < n && b[i] == c {
                    i += 1;
                }
                line += lines(start, i);
            }
            b'`' => {
                i = skip_interpolated(b, i + 1, &mut interp);
                line += lines(start, i);
            }
            b'}' if interp.last() == Some(&0) => {
                interp.pop();
                i = skip_interpolated(b, i + 1, &mut interp);
                line += lines(start, i);
            }
            // Luau's `readNumber`: digits, `.` and `_`, an exponent sign, then letters.
            b'0'..=b'9' => i = skip_number(b, i),
            b'.' if b.get(i + 1).is_some_and(u8::is_ascii_digit) => i = skip_number(b, i + 1),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                out.push(Token {
                    tok: Tok::Word(&src[start..i]),
                    start,
                    end: i,
                    line,
                });
            }
            b'=' | b'~' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'.'
                if b.get(i + 1) == Some(&b'=') =>
            {
                i += 2;
                out.push(Token {
                    tok: Tok::Op,
                    start,
                    end: i,
                    line,
                });
            }
            _ if c.is_ascii() => {
                match c {
                    b'{' => {
                        if let Some(depth) = interp.last_mut() {
                            *depth += 1;
                        }
                    }
                    b'}' => {
                        if let Some(depth) = interp.last_mut() {
                            *depth -= 1;
                        }
                    }
                    _ => {}
                }
                i += 1;
                out.push(Token {
                    tok: Tok::Punct(c),
                    start,
                    end: i,
                    line,
                });
            }
            // Bytes of non-ASCII text: never part of a name.
            _ => i += 1,
        }
    }
    out
}

/// The level of a long bracket (`[[`, `[=[` ...) opening at `i`, if one does.
fn long_bracket(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let level = b[i + 1..].iter().take_while(|&&c| c == b'=').count();
    (b.get(i + 1 + level) == Some(&b'[')).then_some(level)
}

/// Just past the long bracket of `level` that opens at `i` and its closing `]=*]` with
/// exactly `level` equals signs (Luau's `readLongString`).
fn skip_long(b: &[u8], i: usize, level: usize) -> usize {
    let mut j = i + level + 2;
    while j < b.len() {
        if b[j] == b']' {
            let eq = b[j + 1..].iter().take_while(|&&c| c == b'=').count();
            if eq == level && b.get(j + 1 + eq) == Some(&b']') {
                return j + eq + 2;
            }
            j += 1 + eq;
        } else {
            j += 1;
        }
    }
    b.len()
}

/// Just past the backslash escape at `i` (Luau's `readBackslashInString`): `\z` skips the
/// whitespace after it, line breaks included, and a backslash keeps a line break.
fn skip_escape(b: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    match b.get(j) {
        None | Some(0) => j,
        Some(b'\r') => {
            j += 1;
            if b.get(j) == Some(&b'\n') {
                j += 1;
            }
            j
        }
        Some(b'z') => {
            j += 1;
            while b
                .get(j)
                .is_some_and(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c))
            {
                j += 1;
            }
            j
        }
        Some(_) => j + 1,
    }
}

/// Just past the number starting at `i` (Luau's `readNumber`).
fn skip_number(b: &[u8], mut i: usize) -> usize {
    while b
        .get(i)
        .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'.' | b'_'))
    {
        i += 1;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
    }
    while b
        .get(i)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
    {
        i += 1;
    }
    i
}

/// Skips an interpolated string's text from `i` to its closing backtick, or to a `{` that
/// starts an expression (pushed onto `interp`); Luau's `readInterpolatedStringSection`.
fn skip_interpolated(b: &[u8], mut i: usize, interp: &mut Vec<u32>) -> usize {
    while i < b.len() {
        match b[i] {
            b'`' => return i + 1,
            // An unescaped line break: Luau reports the broken string.
            b'\n' | b'\r' | 0 => return i,
            b'\\' if b[i + 1..].starts_with(b"u{") => i += 3,
            b'\\' => i = skip_escape(b, i),
            b'{' => {
                interp.push(0);
                return i + 1;
            }
            _ => i += 1,
        }
    }
    b.len()
}

/// Every generic `for` loop in `src`.
///
/// Fails if `src` contains [`ITERATE`] anywhere, even in a string or comment (a plain text
/// search, so every use of the name in the compiled code is one the rewrite added as the
/// callee of a call), or if a loop's `do` can't be found (then `src` is either not valid
/// Luau, or a loop the rewrite can't place).
pub(crate) fn loops(src: &str) -> Result<Vec<Loop>, Problem> {
    if let Some(at) = src.find(ITERATE) {
        let line = src[..at].bytes().filter(|&c| c == b'\n').count() + 1;
        return Err(Problem {
            line: u32::try_from(line).ok(),
            message: format!(
                "`{ITERATE}` is reserved for the engine and can't appear in a script, even in \
                 a string or comment; use another name"
            ),
        });
    }
    let toks = tokens(src);
    let mut found = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.tok != Tok::Word("for") {
            continue;
        }
        // The loop's names (with any type annotations), up to `in` or `=`.
        let mut depth = 0i32;
        let mut at_in = None;
        for (j, h) in toks.iter().enumerate().skip(i + 1) {
            match h.tok {
                Tok::Punct(b'(' | b'[' | b'{') => depth += 1,
                Tok::Punct(b')' | b']' | b'}') => depth -= 1,
                Tok::Word("in") if depth == 0 => {
                    at_in = Some(j);
                    break;
                }
                Tok::Punct(b'=') | Tok::Word("do") if depth == 0 => break,
                _ => {}
            }
        }
        let Some(j) = at_in else { continue };
        // The iterator expressions, up to the loop's `do`.
        let (mut brackets, mut blocks) = (0i32, 0i32);
        let mut at_do = None;
        for (k, e) in toks.iter().enumerate().skip(j + 1) {
            match e.tok {
                Tok::Punct(b'(' | b'[' | b'{') => brackets += 1,
                Tok::Punct(b')' | b']' | b'}') => brackets -= 1,
                Tok::Word(w) if brackets == 0 => match w {
                    "do" if blocks == 0 => {
                        at_do = Some(k);
                        break;
                    }
                    // Blocks inside a function literal. At the top, `if` is an
                    // if-expression, which has no `end`.
                    "function" | "do" | "repeat" => blocks += 1,
                    "if" if blocks > 0 => blocks += 1,
                    "end" | "until" => blocks -= 1,
                    _ => {}
                },
                _ => {}
            }
            if brackets < 0 || blocks < 0 {
                break;
            }
        }
        match at_do {
            // `for x in do`: nothing to wrap; Luau reports it.
            Some(k) if k == j + 1 => {}
            Some(k) => found.push(Loop {
                line: t.line,
                open: toks[j].end,
                close: toks[k].start,
            }),
            None => return Err(unplaced(t.line)),
        }
    }
    Ok(found)
}

fn unplaced(line: u32) -> Problem {
    Problem {
        line: Some(line),
        message: format!(
            "can't find where this `for ... in` loop's iterator ends, so its order can't be \
             checked (the engine wraps it in `{ITERATE}(...)`); put the iterator in \
             parentheses"
        ),
    }
}

/// `src` with each loop's iterator expressions wrapped in a call to [`ITERATE`].
pub(crate) fn wrap(src: &str, loops: &[Loop]) -> String {
    let mut cuts: Vec<(usize, &str)> = Vec::with_capacity(loops.len() * 2);
    let call = format!(" {ITERATE}(");
    for l in loops {
        cuts.push((l.open, call.as_str()));
        cuts.push((l.close, ")"));
    }
    cuts.sort_by_key(|&(at, _)| at);
    let mut out = String::with_capacity(src.len() + cuts.len() * (ITERATE.len() + 2));
    let mut from = 0;
    for (at, text) in cuts {
        out.push_str(&src[from..at]);
        out.push_str(text);
        from = at;
    }
    out.push_str(&src[from..]);
    out
}

/// The loop whose wrapping broke a source that compiles unwrapped: the first one that
/// doesn't compile wrapped on its own.
pub(crate) fn culprit(src: &str, loops: &[Loop], compiles: impl Fn(&str) -> bool) -> Problem {
    let line = loops
        .iter()
        .find(|l| !compiles(&wrap(src, std::slice::from_ref(l))))
        .or(loops.first())
        .map_or(1, |l| l.line);
    unplaced(line)
}

/// Confirms, on the compiled `bytecode` of a wrapped source, that every generic `for` loop
/// takes its iterator from [`ITERATE`] ([`crate::bytecode`]). This doesn't rely on the
/// scanner above: a loop it missed, or bytecode the check can't read, is a load error.
pub(crate) fn confirm(bytecode: &[u8]) -> Result<(), Problem> {
    match crate::bytecode::unguarded(bytecode, ITERATE) {
        Ok(missed) => match missed.first() {
            None => Ok(()),
            Some(m) => Err(Problem {
                line: m.line,
                message: format!(
                    "the engine couldn't confirm that this `for ... in` loop goes through its \
                     iteration-order check (`{ITERATE}`), so the file isn't loaded rather than \
                     risk an order that changes from run to run. This is an engine bug: report \
                     it with this file"
                ),
            }),
        },
        Err(why) => Err(Problem {
            line: None,
            message: format!(
                "the engine couldn't read the compiled code to confirm that every `for ... in` \
                 loop goes through its iteration-order check ({why}), so the file isn't \
                 loaded. This is an engine bug: report it with this file"
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrapped(src: &str) -> String {
        wrap(src, &loops(src).unwrap())
    }

    #[test]
    fn wraps_generic_loops_only() {
        assert_eq!(
            wrapped("for k, v in t do end"),
            "for k, v in __galaxy_iterate( t )do end"
        );
        assert_eq!(
            wrapped("for k, v in pairs(t) do end"),
            "for k, v in __galaxy_iterate( pairs(t) )do end"
        );
        assert_eq!(
            wrapped("for i, v in next, t, nil do end"),
            "for i, v in __galaxy_iterate( next, t, nil )do end"
        );
        assert_eq!(wrapped("for i = 1, #t do end"), "for i = 1, #t do end");
        assert_eq!(
            wrapped("for i: number = 1, 2 do end"),
            "for i: number = 1, 2 do end"
        );
        assert_eq!(
            wrapped("for k: string, v: { [string]: number } in t do end"),
            "for k: string, v: { [string]: number } in __galaxy_iterate( t )do end"
        );
    }

    #[test]
    fn skips_strings_and_comments() {
        let src = "local s = 'for a in b do' .. \"for c in d do\" .. [==[for e in f do]==]
            -- for g in h do
            --[[ for i in j do ]]
            local x = `for {y} in z do`";
        assert_eq!(wrapped(src), src);
    }

    #[test]
    fn follows_luaus_string_rules() {
        // `\z` skips whitespace across lines; a backslash keeps a line break; `\u{` in an
        // interpolated string isn't an expression; a long bracket closes only at its level.
        for (src, line) in [
            ("local s = \"\\z\n\"; for k in t do end", 2),
            ("local s = '\\z  \r\n\t  '; for k in t do end", 2),
            ("local s = 'a\\\nb'; for k in t do end", 2),
            ("local s = 'a\\\r\nb'; for k in t do end", 2),
            ("local s = `\\u{48}\\z\n`; for k in t do end", 2),
            ("local s = `{'\\z\n'}`; for k in t do end", 2),
            ("local s = [=[ ]] ]==] ]=]; for k in t do end", 1),
            ("--[==[ ]=] ]==] for k in t do end", 1),
            ("local n = 1e+5 + .5 + 0x1F; for k in t do end", 1),
        ] {
            let found = loops(src).unwrap();
            assert_eq!(found.len(), 1, "{src:?}");
            assert_eq!(found[0].line, line, "{src:?}");
        }
        assert_eq!(
            wrapped("local s = \"\\z\n\"; for k in t do end"),
            "local s = \"\\z\n\"; for k in __galaxy_iterate( t )do end"
        );
    }

    #[test]
    fn a_loop_the_scanner_missed_fails_the_bytecode_check() {
        let compile = |s: &str| mlua::chunk::Compiler::new().compile(s).unwrap();
        // The source as the old scanner left it: the loop after `\z` unwrapped.
        let missed = "local t = {}\nlocal s = \"\\z\n\"; for k in t do end";
        let problem = confirm(&compile(missed)).unwrap_err();
        assert_eq!(problem.line, Some(3), "{problem:?}");
        assert!(problem.message.contains("couldn't confirm"), "{problem:?}");
        assert_eq!(confirm(&compile(&wrapped(missed))), Ok(()));
    }

    #[test]
    fn finds_the_loops_do() {
        // Brackets, a function literal with its own blocks, nested loops, if-expressions.
        assert_eq!(
            wrapped("for k in f({ a = 1 }, t[1]) do end"),
            "for k in __galaxy_iterate( f({ a = 1 }, t[1]) )do end"
        );
        assert_eq!(
            wrapped("for k in function() while x do end repeat until y end do end"),
            "for k in __galaxy_iterate( function() while x do end repeat until y end )do end"
        );
        assert_eq!(
            wrapped("for k in if c then a else b do for v in k do end end"),
            "for k in __galaxy_iterate( if c then a else b )do for v in __galaxy_iterate( k )do end end"
        );
        // A loop inside an interpolated string's expression.
        assert_eq!(
            wrapped("local s = `{(function() for k in t do end end)()}`"),
            "local s = `{(function() for k in __galaxy_iterate( t )do end end)()}`"
        );
    }

    #[test]
    fn keeps_lines() {
        let src = "local a = 1\nfor k\n, v in\n t\n do\nend";
        let out = wrapped(src);
        assert_eq!(out.lines().count(), src.lines().count());
        assert_eq!(loops(src).unwrap()[0].line, 2);
    }

    #[test]
    fn reports_what_it_cant_wrap() {
        let reserved = loops("local x = 1\nlocal __galaxy_iterate = 2").unwrap_err();
        assert_eq!(reserved.line, Some(2));
        let reserved = loops("local x = 1\n\n-- __galaxy_iterate").unwrap_err();
        assert_eq!(reserved.line, Some(3));
        // An if-expression inside a function literal looks like an `if` block.
        let src = "\nfor k in function() return if a then b else c end do end";
        assert_eq!(loops(src).unwrap_err().line, Some(2));
        // In parentheses it's found.
        assert!(loops("for k in (function() return if a then b else c end) do end").is_ok());
        // Nothing to wrap is left for Luau to report.
        assert_eq!(wrapped("for k in do end"), "for k in do end");
    }
}

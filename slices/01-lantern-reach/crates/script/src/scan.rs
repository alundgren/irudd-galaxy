//! A small Luau tokenizer and the static checks that run on its tokens.
//!
//! These checks don't need the Luau analyzer, so they always run and give precise
//! file:line locations: hooks used against the manifest's `uses`, clock and timer
//! APIs, unseeded randomness, forbidden globals, lines written without `cites`, loops
//! whose order isn't stable, and string literals that look like world IDs.

use std::collections::BTreeSet;

use galaxy_contract::api;
use galaxy_contract::pack::Diagnostic;

use crate::pack::error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    Name(String),
    Str(String),
    Num,
    Punct(String),
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
}

impl Token {
    fn is(&self, p: &str) -> bool {
        matches!(&self.tok, Tok::Punct(x) if x == p)
    }
    fn name(&self) -> Option<&str> {
        match &self.tok {
            Tok::Name(n) => Some(n),
            _ => None,
        }
    }
}

/// Splits Luau source into tokens, dropping comments and whitespace. Unterminated
/// strings and comments end at the end of the file (the parser reports them).
pub fn tokenize(src: &str) -> Vec<Token> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            line += 1;
            i += 1;
        } else if c.is_ascii_whitespace() {
            i += 1;
        } else if b[i..].starts_with(b"--") {
            i += 2;
            if let Some(level) = long_bracket(&b[i..]) {
                let (end, lines) = skip_long(b, i + level + 2, level);
                line += lines;
                i = end;
            } else {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
        } else if c == b'[' && long_bracket(&b[i..]).is_some() {
            let level = long_bracket(&b[i..]).unwrap_or(0);
            let start_line = line;
            let body_start = i + level + 2;
            let (end, lines) = skip_long(b, body_start, level);
            let body_end = end.saturating_sub(level + 2).max(body_start);
            line += lines;
            out.push(Token {
                tok: Tok::Str(src[body_start..body_end].to_owned()),
                line: start_line,
            });
            i = end;
        } else if c == b'"' || c == b'\'' || c == b'`' {
            let start_line = line;
            let mut s = String::new();
            i += 1;
            while i < b.len() && b[i] != c {
                if b[i] == b'\\' && i + 1 < b.len() {
                    if b[i + 1] == b'\n' {
                        line += 1;
                    }
                    s.push(b[i + 1] as char);
                    i += 2;
                    continue;
                }
                if b[i] == b'\n' {
                    line += 1;
                    if c != b'`' {
                        break;
                    }
                }
                s.push(b[i] as char);
                i += 1;
            }
            i += 1;
            out.push(Token {
                tok: Tok::Str(s),
                line: start_line,
            });
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Token {
                tok: Tok::Name(src[start..i].to_owned()),
                line,
            });
        } else if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
            out.push(Token {
                tok: Tok::Num,
                line,
            });
        } else {
            let three = src.get(i..i + 3).unwrap_or("");
            let two = src.get(i..i + 2).unwrap_or("");
            let p = if three == "..." || three == "..=" || three == "//=" {
                three
            } else if [
                "==", "~=", "<=", ">=", "..", "::", "->", "+=", "-=", "*=", "/=", "%=", "^=", "//",
            ]
            .contains(&two)
            {
                two
            } else {
                src.get(i..i + 1).unwrap_or("?")
            };
            i += p.len().max(1);
            out.push(Token {
                tok: Tok::Punct(p.to_owned()),
                line,
            });
        }
    }
    out
}

/// `[[`, `[=[`, ... at the start: the number of `=`.
fn long_bracket(b: &[u8]) -> Option<usize> {
    if b.first() != Some(&b'[') {
        return None;
    }
    let eq = b[1..].iter().take_while(|&&c| c == b'=').count();
    (b.get(1 + eq) == Some(&b'[')).then_some(eq)
}

/// Skips to after the matching `]=*]`. Returns the end index and newlines crossed.
fn skip_long(b: &[u8], mut i: usize, level: usize) -> (usize, u32) {
    let mut lines = 0;
    while i < b.len() {
        if b[i] == b'\n' {
            lines += 1;
        }
        if b[i] == b']'
            && b[i + 1..].iter().take(level).all(|&c| c == b'=')
            && b.get(i + 1 + level) == Some(&b']')
        {
            return (i + level + 2, lines);
        }
        i += 1;
    }
    (b.len(), lines)
}

/// Names declared as locals or parameters anywhere in the file (so `local time = 3`
/// isn't mistaken for the Roblox `time()` global).
fn declared_names(toks: &[Token]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut i = 0;
    while i < toks.len() {
        match toks[i].name() {
            Some("local") => {
                let mut j = i + 1;
                if toks.get(j).and_then(Token::name) == Some("function") {
                    j += 1;
                }
                while let Some(n) = toks.get(j).and_then(Token::name) {
                    names.insert(n.to_owned());
                    j += 1;
                    // Skip a type annotation up to the next `,` or `=`.
                    if toks.get(j).is_some_and(|t| t.is(":")) {
                        while toks.get(j).is_some_and(|t| !t.is(",") && !t.is("=")) {
                            j += 1;
                        }
                    }
                    if toks.get(j).is_some_and(|t| t.is(",")) {
                        j += 1;
                    } else {
                        break;
                    }
                }
            }
            Some("function") => {
                let mut j = i + 1;
                while toks.get(j).is_some_and(|t| !t.is("(")) {
                    j += 1;
                }
                let mut depth = 0;
                while let Some(t) = toks.get(j) {
                    if t.is("(") {
                        depth += 1;
                    } else if t.is(")") {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if depth == 1
                        && let Some(n) = t.name()
                        && toks.get(j - 1).is_some_and(|p| p.is("(") || p.is(","))
                    {
                        names.insert(n.to_owned());
                    }
                    j += 1;
                }
            }
            Some("for") => {
                let mut j = i + 1;
                while let Some(n) = toks.get(j).and_then(Token::name) {
                    if n == "in" {
                        break;
                    }
                    names.insert(n.to_owned());
                    j += 1;
                    if toks.get(j).is_some_and(|t| t.is(",")) {
                        j += 1;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    names
}

/// What the static scan of one file found.
#[derive(Debug, Default)]
pub struct Scan {
    /// `(namespace, function, line)` for every `ctx.<ns>:<fn>` / `ctx:<fn>`.
    pub hook_calls: Vec<(String, String, u32)>,
    /// `(namespace, line)` for `ctx.<ns>` without a recognisable call.
    pub namespaces: Vec<(String, u32)>,
    /// String literals that look like IDs: `(id, line)`.
    pub ids: Vec<(String, u32)>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Global names that read a clock or schedule by time (Luau's `os`, Roblox timers).
const TIMER_GLOBALS: &[&str] = &[
    "wait",
    "delay",
    "spawn",
    "tick",
    "time",
    "elapsedTime",
    "task",
];
/// Globals the sandbox removes.
const FORBIDDEN_GLOBALS: &[&str] = &[
    "loadstring",
    "load",
    "dofile",
    "loadfile",
    "getfenv",
    "setfenv",
    "collectgarbage",
    "gcinfo",
    "io",
    "debug",
    "package",
];

/// ID prefixes the reference check knows how to resolve.
pub const ID_KINDS: &[&str] = &[
    "station",
    "npc",
    "faction",
    "region",
    "ware",
    "rumor",
    "ship",
    "lane",
    "hull",
    "field",
    "rock_type",
];

/// Scans one Luau file.
pub fn scan(file: &str, src: &str) -> Scan {
    let toks = tokenize(src);
    let declared = declared_names(&toks);
    let mut out = Scan::default();
    let at = Some(file);
    for (i, t) in toks.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|p| toks.get(p));
        let next = toks.get(i + 1);
        let after_dot = prev.is_some_and(|p| p.is(".") || p.is(":"));
        if let Tok::Str(s) = &t.tok {
            if let Some((kind, _)) = s.split_once('.')
                && ID_KINDS.contains(&kind)
                && galaxy_contract::ids::is_valid_id(s, 2)
            {
                out.ids.push((s.clone(), t.line));
            }
            continue;
        }
        let Some(name) = t.name() else { continue };
        if after_dot {
            continue;
        }
        if name == "for" {
            if let Some(d) = unordered_iteration(&toks, i, &declared, file) {
                out.diagnostics.push(d);
            }
            continue;
        }
        // ctx.<ns>:<fn>, ctx.<ns>.<fn>, ctx:<fn>
        if name == "ctx" {
            // `ctx:cause(...)` is a call; `ctx: G.Ctx` is a type annotation.
            let called = |j: usize| {
                toks.get(j)
                    .is_some_and(|t| t.is("(") || t.is("{") || matches!(t.tok, Tok::Str(_)))
            };
            if next.is_some_and(|n| n.is(":"))
                && let Some(f) = toks.get(i + 2).and_then(Token::name)
                && called(i + 3)
            {
                out.hook_calls.push((String::new(), f.to_owned(), t.line));
            } else if next.is_some_and(|n| n.is(":")) {
                // A type annotation.
            } else if next.is_some_and(|n| n.is("."))
                && let Some(ns) = toks.get(i + 2).and_then(Token::name)
            {
                let sep = toks.get(i + 3);
                if sep.is_some_and(|s| s.is(":") || s.is("."))
                    && let Some(f) = toks.get(i + 4).and_then(Token::name)
                {
                    out.hook_calls.push((ns.to_owned(), f.to_owned(), t.line));
                    if ["talk", "map", "chronicle"].contains(&ns) {
                        check_cites(&toks, i + 5, file, t.line, &mut out.diagnostics);
                    }
                } else {
                    out.namespaces.push((ns.to_owned(), t.line));
                }
            }
            continue;
        }
        if declared.contains(name)
            || prev.is_some_and(|p| p.name() == Some("local") || p.name() == Some("function"))
        {
            continue;
        }
        // A table key (`{ time = 1 }`) isn't a global read.
        if next.is_some_and(|n| n.is("=")) && prev.is_some_and(|p| p.is("{") || p.is(",")) {
            continue;
        }
        if name == "os" && next.is_some_and(|n| n.is(".")) {
            let f = toks.get(i + 2).and_then(Token::name).unwrap_or("?");
            out.diagnostics.push(error(
                "clock_api",
                format!("`os.{f}` reads the clock; scripts pace by activity counters (`G.after`, `ctx.counters`), never time"),
                at,
                Some(t.line),
            ));
        } else if name == "math"
            && next.is_some_and(|n| n.is("."))
            && let Some(f @ ("random" | "randomseed")) = toks.get(i + 2).and_then(Token::name)
        {
            out.diagnostics.push(error(
                "unseeded_random",
                format!("`math.{f}` isn't available; use the seeded `ctx.rng`"),
                at,
                Some(t.line),
            ));
        } else if TIMER_GLOBALS.contains(&name) && next.is_some_and(|n| n.is("(") || n.is(".")) {
            out.diagnostics.push(error(
                "clock_api",
                format!("`{name}` is a timer or clock API; scripts pace by activity counters, never time"),
                at,
                Some(t.line),
            ));
        } else if FORBIDDEN_GLOBALS.contains(&name) && next.is_some_and(|n| n.is("(") || n.is("."))
        {
            out.diagnostics.push(error(
                "forbidden_global",
                format!("`{name}` isn't available in the sandbox"),
                at,
                Some(t.line),
            ));
        } else if name == "require"
            && let Some(Tok::Str(path)) = next
                .filter(|n| n.is("("))
                .and_then(|_| toks.get(i + 2))
                .or(next)
                .map(|t| &t.tok)
            && path != "@galaxy"
            && !path.starts_with("./")
            && !path.starts_with("../")
        {
            out.diagnostics.push(error(
                "bad_require",
                format!("`require(\"{path}\")`: only `@galaxy` and the pack's own modules (`./name`) can be required"),
                at,
                Some(t.line),
            ));
        }
    }
    out
}

/// `for ... in <iterator> do`: the iterator must be one whose order is the same on every
/// run. A bare `for k, v in t` walks the table in its internal order, which follows
/// heap addresses for keys that are tables or functions, and can't be intercepted at
/// runtime; the sandbox's `pairs` and `next` refuse such tables, `ipairs`,
/// `string.gmatch` and `utf8.codes` are ordered. Shadowing one of those names makes it
/// unknown, so it's refused too.
fn unordered_iteration(
    toks: &[Token],
    at: usize,
    declared: &BTreeSet<String>,
    file: &str,
) -> Option<Diagnostic> {
    let mut j = at + 1;
    loop {
        let t = toks.get(j)?;
        if t.is("=") || t.name() == Some("do") {
            return None; // a numeric `for`
        }
        if t.name() == Some("in") {
            break;
        }
        j += 1;
    }
    let tok = |k: usize| toks.get(j + k);
    let known = |n: &str| !declared.contains(n);
    let ordered = match tok(1).and_then(Token::name) {
        Some(f @ ("pairs" | "ipairs")) => known(f) && tok(2).is_some_and(|t| t.is("(")),
        Some("next") => known("next") && tok(2).is_some_and(|t| t.is(",")),
        Some(lib @ ("string" | "utf8")) => {
            known(lib)
                && tok(2).is_some_and(|t| t.is("."))
                && matches!(
                    (lib, tok(3).and_then(Token::name)),
                    ("string", Some("gmatch")) | ("utf8", Some("codes"))
                )
        }
        _ => false,
    };
    (!ordered).then(|| {
        error(
            "unordered_iteration",
            "iterate with `pairs(t)`, `ipairs(t)`, `next`, `string.gmatch` or `utf8.codes`: \
             a bare `for k, v in t` has no stable order when keys are tables or functions",
            Some(file),
            Some(toks[at].line),
        )
    })
}

/// After `ctx.talk:offer`: if the argument is a table constructor, it must have a
/// top-level `cites` key.
fn check_cites(toks: &[Token], mut j: usize, file: &str, line: u32, out: &mut Vec<Diagnostic>) {
    if toks.get(j).is_some_and(|t| t.is("(")) {
        j += 1;
    }
    if !toks.get(j).is_some_and(|t| t.is("{")) {
        return; // A variable: checked at runtime.
    }
    let mut depth = 0;
    let mut has_cites = false;
    while let Some(t) = toks.get(j) {
        if t.is("{") || t.is("(") || t.is("[") {
            depth += 1;
        } else if t.is("}") || t.is(")") || t.is("]") {
            depth -= 1;
            if depth == 0 {
                break;
            }
        } else if depth == 1
            && t.name() == Some("cites")
            && toks.get(j + 1).is_some_and(|n| n.is("="))
        {
            has_cites = true;
        }
        j += 1;
    }
    if !has_cites {
        out.push(error(
            "uncited_line",
            "this line has no `cites`: every talk line, map note and chronicle entry must cite a history fact ID",
            Some(file),
            Some(line),
        ));
    }
}

/// Checks a file's hook calls against the declared capabilities.
pub fn hook_diagnostics(
    file: &str,
    scan: &Scan,
    uses: &[String],
    has_stores: bool,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let declared = |u: &str| uses.iter().any(|x| x == u);
    for (ns, f, line) in &scan.hook_calls {
        if ns == "store" && !has_stores {
            out.push(error(
                "undeclared_store",
                "`ctx.store` is only there for scripts that declare stores in the manifest",
                Some(file),
                Some(*line),
            ));
            continue;
        }
        let Some(def) = api::function(ns, f) else {
            let what = if ns.is_empty() {
                format!("ctx:{f}")
            } else {
                format!("ctx.{ns}:{f}")
            };
            out.push(error(
                "unknown_hook",
                format!("`{what}` isn't part of the hook API (see crates/contract/api/HOOKS.md)"),
                Some(file),
                Some(*line),
            ));
            continue;
        };
        if !def.needs.is_empty() && !declared(def.needs) {
            out.push(error(
                "undeclared_hook",
                format!(
                    "`ctx.{ns}:{f}` needs `{}` in the manifest's `uses`",
                    def.needs
                ),
                Some(file),
                Some(*line),
            ));
        }
    }
    for (ns, line) in &scan.namespaces {
        if ["pack", "script"].contains(&ns.as_str()) {
            continue;
        }
        if !api::NAMESPACES.iter().any(|(n, _)| n == ns) {
            out.push(error(
                "unknown_hook",
                format!("`ctx.{ns}` isn't part of the hook API"),
                Some(file),
                Some(*line),
            ));
        } else if !api::FUNCTIONS
            .iter()
            .any(|d| d.ns == ns && (d.needs.is_empty() || declared(d.needs)))
        {
            out.push(error(
                "undeclared_hook",
                format!("`ctx.{ns}` isn't available: no capability for it is declared in `uses`"),
                Some(file),
                Some(*line),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_track_lines_and_skip_comments() {
        let src = "-- a comment\nlocal x = \"station.kettle_dock\" --[[ block\n comment ]]\nctx.stations:get(x)\n";
        let s = scan("f.luau", src);
        assert_eq!(s.ids, vec![("station.kettle_dock".to_owned(), 2)]);
        assert_eq!(s.hook_calls, vec![("stations".into(), "get".into(), 4)]);
    }

    #[test]
    fn clock_and_random_are_flagged() {
        let src =
            "local a = os.time()\nlocal b = math.random(1, 2)\nwait(3)\nlocal t = { time = 3 }\n";
        let s = scan("f.luau", src);
        let codes: Vec<_> = s
            .diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.location.as_ref().unwrap().line))
            .collect();
        assert_eq!(
            codes,
            vec![
                ("clock_api", Some(1)),
                ("unseeded_random", Some(2)),
                ("clock_api", Some(3))
            ]
        );
    }

    #[test]
    fn local_names_shadow_timer_globals() {
        let s = scan(
            "f.luau",
            "local function time(x) return x end\nprint(time(3))\n",
        );
        assert!(s.diagnostics.is_empty(), "{:?}", s.diagnostics);
    }

    #[test]
    fn lines_need_cites() {
        let ok = scan(
            "f.luau",
            "ctx.talk:offer({ speaker = \"npc.a\", text = \"hi\", cites = { ev.fact } }, why)\n",
        );
        assert!(ok.diagnostics.is_empty());
        let bad = scan(
            "f.luau",
            "\nctx.talk:offer { speaker = \"npc.a\", text = \"hi\", about = { cites = 1 } }\n",
        );
        assert_eq!(bad.diagnostics[0].code, "uncited_line");
        assert_eq!(bad.diagnostics[0].location.as_ref().unwrap().line, Some(2));
    }

    #[test]
    fn loops_must_iterate_in_a_stable_order() {
        let ok = scan(
            "f.luau",
            "for i = 1, 3 do end\nfor k, v in pairs(t) do end\nfor _, x in ipairs({ 1 }) do end\n\
             for k in next, t do end\nfor w in string.gmatch(s, \"%a+\") do end\n\
             for k: string, v: number in pairs(t) do end\n",
        );
        assert!(ok.diagnostics.is_empty(), "{:?}", ok.diagnostics);
        let bad = scan(
            "f.luau",
            "for k, v in t do end\nfor _, x in { 1, 2 } do end\nfor k in f() do end\n\
             local function pairs(x) return x end\nfor k in pairs(t) do end\n",
        );
        let lines: Vec<_> = bad
            .diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.location.as_ref().unwrap().line))
            .collect();
        assert_eq!(
            lines,
            vec![
                ("unordered_iteration", Some(1)),
                ("unordered_iteration", Some(2)),
                ("unordered_iteration", Some(3)),
                ("unordered_iteration", Some(5)),
            ]
        );
    }

    #[test]
    fn undeclared_hooks_are_reported() {
        let s = scan(
            "f.luau",
            "ctx.stations:get(a)\nctx.stations:set_stage(a, 2, why)\nctx.market:nope()\n",
        );
        let d = hook_diagnostics("f.luau", &s, &["stations.read".into()], false);
        let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes, vec!["undeclared_hook", "unknown_hook"]);
    }
}

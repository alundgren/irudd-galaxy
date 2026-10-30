//! A small Luau tokenizer and the static checks that run on its tokens.
//!
//! These checks don't need the Luau analyzer, so they always run and give precise
//! file:line locations: hooks used against the manifest's `uses`, clock and timer
//! APIs, unseeded randomness, forbidden globals, lines written without `cites`, loops
//! whose order isn't stable, and string literals that look like world IDs.
//!
//! The tokenizer follows Luau's lexer (`Ast/src/Lexer.cpp`) wherever a difference
//! could hide code from the checks: quoted strings and their escapes (`\z`, a
//! backslash before a line break), long strings and comments of any level, line
//! comments that end at `\r` as well as `\n`, and interpolated strings with nested
//! `{ ... }` expressions and `\u{...}` escapes. A file the lexer would reject doesn't
//! run, so how the tokenizer reads one doesn't matter.

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
    fn is_name(&self, n: &str) -> bool {
        self.name() == Some(n)
    }
}

/// Luau's reserved words.
const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local",
    "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

fn is_keyword(n: &str) -> bool {
    KEYWORDS.contains(&n)
}

/// What a `{` opened: a table or block brace, or an interpolated string's expression.
#[derive(Clone, Copy, PartialEq)]
enum Brace {
    Normal,
    Interp,
}

struct Lexer<'a> {
    src: &'a str,
    b: &'a [u8],
    i: usize,
    line: u32,
    out: Vec<Token>,
    braces: Vec<Brace>,
}

impl Lexer<'_> {
    fn peek(&self, k: usize) -> u8 {
        self.b.get(self.i + k).copied().unwrap_or(0)
    }

    /// Consumes one byte, counting lines the way Luau does (`\n` only).
    fn bump(&mut self) {
        if self.peek(0) == b'\n' {
            self.line += 1;
        }
        self.i += 1;
    }

    fn push(&mut self, tok: Tok, line: u32) {
        self.out.push(Token { tok, line });
    }

    /// At `[` or `]`: `Some(level)` for `[=*[` / `]=*]`, without consuming.
    fn long_bracket(&self) -> Option<usize> {
        let open = self.peek(0);
        let eq = self.b[self.i + 1..]
            .iter()
            .take_while(|&&c| c == b'=')
            .count();
        (self.peek(1 + eq) == open).then_some(eq)
    }

    /// From `[=*[`: skips to after the matching `]=*]` (or the end) and returns the
    /// body.
    fn long_string(&mut self, level: usize) -> String {
        self.i += level + 2;
        let start = self.i;
        while self.i < self.b.len() {
            if self.peek(0) == b']' && self.long_bracket() == Some(level) {
                let body = self.src.get(start..self.i).unwrap_or("").to_owned();
                self.i += level + 2;
                return body;
            }
            self.bump();
        }
        self.src.get(start..).unwrap_or("").to_owned()
    }

    /// At a backslash in a string: skips the escape like `readBackslashInString`.
    fn escape(&mut self, text: &mut Vec<u8>) {
        self.i += 1;
        match self.peek(0) {
            b'\r' => {
                self.i += 1;
                if self.peek(0) == b'\n' {
                    self.bump();
                }
            }
            0 => {}
            b'z' => {
                self.i += 1;
                while self.peek(0).is_ascii_whitespace() || self.peek(0) == 0x0b {
                    self.bump();
                }
            }
            c => {
                text.push(c);
                self.bump();
            }
        }
    }

    /// A quoted string, from its opening quote. Ends at the quote, or (broken) at a
    /// line break or the end.
    fn quoted(&mut self) {
        let line = self.line;
        let quote = self.peek(0);
        self.i += 1;
        let mut text = Vec::new();
        loop {
            match self.peek(0) {
                0 | b'\r' | b'\n' => break,
                b'\\' => self.escape(&mut text),
                c if c == quote => {
                    self.i += 1;
                    break;
                }
                c => {
                    text.push(c);
                    self.i += 1;
                }
            }
        }
        self.push(Tok::Str(String::from_utf8_lossy(&text).into_owned()), line);
    }

    /// One literal section of an interpolated string, after its opening `` ` `` or `}`.
    /// Ends at `` ` `` (the string is done) or `{` (an expression starts).
    fn interp_section(&mut self) {
        let line = self.line;
        let mut text = Vec::new();
        loop {
            match self.peek(0) {
                0 | b'\r' | b'\n' => break,
                b'`' => {
                    self.i += 1;
                    break;
                }
                b'\\' if self.peek(1) == b'u' && self.peek(2) == b'{' => {
                    text.extend_from_slice(b"\\u{");
                    self.i += 3;
                }
                b'\\' => self.escape(&mut text),
                b'{' => {
                    self.braces.push(Brace::Interp);
                    self.i += 1;
                    break;
                }
                c => {
                    text.push(c);
                    self.i += 1;
                }
            }
        }
        self.push(Tok::Str(String::from_utf8_lossy(&text).into_owned()), line);
    }

    fn run(mut self) -> Vec<Token> {
        while self.i < self.b.len() {
            let c = self.peek(0);
            if c == b'\n' || c.is_ascii_whitespace() || c == 0x0b {
                self.bump();
            } else if c == b'-' && self.peek(1) == b'-' {
                self.i += 2;
                match (self.peek(0) == b'[')
                    .then(|| self.long_bracket())
                    .flatten()
                {
                    Some(level) => {
                        self.long_string(level);
                    }
                    None => {
                        while !matches!(self.peek(0), 0 | b'\r' | b'\n') {
                            self.i += 1;
                        }
                    }
                }
            } else if c == b'['
                && let Some(level) = self.long_bracket()
            {
                let line = self.line;
                let body = self.long_string(level);
                self.push(Tok::Str(body), line);
            } else if c == b'"' || c == b'\'' {
                self.quoted();
            } else if c == b'`' {
                self.i += 1;
                self.interp_section();
            } else if c == b'{' {
                self.braces.push(Brace::Normal);
                self.i += 1;
                self.push(Tok::Punct("{".into()), self.line);
            } else if c == b'}' {
                self.i += 1;
                if self.braces.pop() == Some(Brace::Interp) {
                    self.interp_section();
                } else {
                    self.push(Tok::Punct("}".into()), self.line);
                }
            } else if c.is_ascii_alphabetic() || c == b'_' {
                let start = self.i;
                while self.peek(0).is_ascii_alphanumeric() || self.peek(0) == b'_' {
                    self.i += 1;
                }
                let name = self.src[start..self.i].to_owned();
                self.push(Tok::Name(name), self.line);
            } else if c.is_ascii_digit() {
                while self.peek(0).is_ascii_digit() || matches!(self.peek(0), b'.' | b'_') {
                    self.i += 1;
                }
                if matches!(self.peek(0), b'e' | b'E') {
                    self.i += 1;
                    if matches!(self.peek(0), b'+' | b'-') {
                        self.i += 1;
                    }
                }
                while self.peek(0).is_ascii_alphanumeric() || self.peek(0) == b'_' {
                    self.i += 1;
                }
                self.push(Tok::Num, self.line);
            } else {
                let three = self.src.get(self.i..self.i + 3).unwrap_or("");
                let two = self.src.get(self.i..self.i + 2).unwrap_or("");
                let p = if ["...", "..=", "//="].contains(&three) {
                    three
                } else if [
                    "==", "~=", "<=", ">=", "..", "::", "->", "+=", "-=", "*=", "/=", "%=", "^=",
                    "//",
                ]
                .contains(&two)
                {
                    two
                } else {
                    self.src.get(self.i..self.i + 1).unwrap_or("?")
                };
                self.i += p.len().max(1);
                let p = p.to_owned();
                self.push(Tok::Punct(p), self.line);
            }
        }
        self.out
    }
}

/// Splits Luau source into tokens, dropping comments and whitespace. Each section of
/// an interpolated string is a `Str`; the expressions between them are tokens.
pub fn tokenize(src: &str) -> Vec<Token> {
    Lexer {
        src,
        b: src.as_bytes(),
        i: 0,
        line: 1,
        out: Vec::new(),
        braces: Vec::new(),
    }
    .run()
}

/// The index of the bracket closing the one at `open` (`(`, `[` or `{`), if the
/// brackets in between balance.
fn closing(toks: &[Token], open: usize) -> Option<usize> {
    let mut stack: Vec<&str> = Vec::new();
    for (j, t) in toks.iter().enumerate().skip(open) {
        let Tok::Punct(p) = &t.tok else {
            if stack.is_empty() {
                return None;
            }
            continue;
        };
        match p.as_str() {
            "(" => stack.push(")"),
            "[" => stack.push("]"),
            "{" => stack.push("}"),
            ")" | "]" | "}" => {
                if stack.pop() != Some(p.as_str()) {
                    return None;
                }
                if stack.is_empty() {
                    return Some(j);
                }
            }
            _ if stack.is_empty() => return None,
            _ => {}
        }
    }
    None
}

/// Skips a type annotation starting at `j` (just after its `:`). Stops, without
/// consuming it, at a `,`, `=` or keyword outside any brackets, or at a closing bracket
/// it didn't open (the end of a parameter list). `<` and `>` nest inside types; a `)`,
/// `]` or `}` also closes any `<` still open inside it (`typeof(a < b)`).
fn skip_type(toks: &[Token], mut j: usize) -> usize {
    let mut stack: Vec<&str> = Vec::new();
    while let Some(t) = toks.get(j) {
        match &t.tok {
            Tok::Punct(p) => match p.as_str() {
                "(" | "[" | "{" | "<" => stack.push(p.as_str()),
                ">" => {
                    if stack.last() == Some(&"<") {
                        stack.pop();
                    }
                }
                ")" | "]" | "}" => {
                    let open = match p.as_str() {
                        ")" => "(",
                        "]" => "[",
                        _ => "{",
                    };
                    while stack.last() == Some(&"<") {
                        stack.pop();
                    }
                    if stack.pop() != Some(open) {
                        return j;
                    }
                }
                "," | "=" if stack.is_empty() => return j,
                _ => {}
            },
            Tok::Name(n) if stack.is_empty() && is_keyword(n) => return j,
            _ => {}
        }
        j += 1;
    }
    j
}

/// A list of names with optional type annotations (`a: T, b`), from `j`. Returns the
/// names' token indices and the index after the list.
fn name_list(toks: &[Token], mut j: usize) -> (Vec<usize>, usize) {
    let mut names = Vec::new();
    while let Some(n) = toks.get(j).and_then(Token::name) {
        if is_keyword(n) {
            break;
        }
        names.push(j);
        j += 1;
        if toks.get(j).is_some_and(|t| t.is(":")) {
            j = skip_type(toks, j + 1);
        }
        if toks.get(j).is_some_and(|t| t.is(",")) {
            j += 1;
        } else {
            break;
        }
    }
    (names, j)
}

/// The token indices of every name a file binds: `local` and `const` names, `local
/// function` and `const function` names, parameters (with their types skipped) and
/// `for` variables. It errs on the
/// side of finding too many, never too few.
fn bindings(toks: &[Token]) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        match t.name() {
            Some("local") | Some("const") => {
                let j = i + 1;
                if toks.get(j).is_some_and(|t| t.is_name("function")) {
                    if toks.get(j + 1).and_then(Token::name).is_some() {
                        out.push(j + 1);
                    }
                } else {
                    out.extend(name_list(toks, j).0);
                }
            }
            Some("for") => out.extend(name_list(toks, i + 1).0),
            Some("function") => {
                // The name (`a.b:c`) and generics (`<T>`), then the parameters.
                let mut j = i + 1;
                while toks.get(j).is_some_and(|t| !t.is("(")) {
                    j += 1;
                }
                j += 1;
                while let Some(t) = toks.get(j) {
                    if t.is("...") {
                        j += 1;
                    } else if t.name().is_some_and(|n| !is_keyword(n)) {
                        out.push(j);
                        j += 1;
                    } else {
                        break;
                    }
                    if toks.get(j).is_some_and(|t| t.is(":")) {
                        j = skip_type(toks, j + 1);
                    }
                    if toks.get(j).is_some_and(|t| t.is(",")) {
                        j += 1;
                    } else {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Names declared as locals or parameters anywhere in the file (so `local time = 3`
/// isn't mistaken for the Roblox `time()` global).
fn declared_names(toks: &[Token]) -> BTreeSet<String> {
    bindings(toks)
        .into_iter()
        .filter_map(|j| toks[j].name().map(str::to_owned))
        .collect()
}

/// The iterator functions a generic `for` may call.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Iter {
    Pairs,
    Ipairs,
    Next,
    Gmatch,
    Codes,
}

/// What the loop check knows about a file's names.
struct Iterators {
    declared: BTreeSet<String>,
    /// Locals proven to hold one of the iterators.
    aliases: Vec<(String, Iter)>,
}

impl Iterators {
    fn new(toks: &[Token]) -> Self {
        let declared = declared_names(toks);
        let mut it = Iterators {
            declared,
            aliases: Vec::new(),
        };
        it.aliases = it.find_aliases(toks);
        it
    }

    /// The iterator a global name or `lib.fn` at `j` is, if its library name isn't
    /// shadowed. Returns it and the index after it.
    fn global_at(&self, toks: &[Token], j: usize) -> Option<(Iter, usize)> {
        let name = toks.get(j)?.name()?;
        if self.declared.contains(name) {
            return None;
        }
        let field = || {
            toks.get(j + 1)
                .is_some_and(|t| t.is("."))
                .then(|| toks.get(j + 2).and_then(Token::name))
                .flatten()
        };
        match name {
            "pairs" => Some((Iter::Pairs, j + 1)),
            "ipairs" => Some((Iter::Ipairs, j + 1)),
            "next" => Some((Iter::Next, j + 1)),
            "string" if field() == Some("gmatch") => Some((Iter::Gmatch, j + 3)),
            "utf8" if field() == Some("codes") => Some((Iter::Codes, j + 3)),
            _ => None,
        }
    }

    /// The iterator at `j`: a global one, or a proven alias.
    fn iter_at(&self, toks: &[Token], j: usize) -> Option<(Iter, usize)> {
        if let Some(found) = self.global_at(toks, j) {
            return Some(found);
        }
        let name = toks.get(j)?.name()?;
        self.aliases
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, it)| (*it, j + 1))
    }

    /// `local g = string.gmatch` (or another iterator) is an alias if it is the only
    /// place `g` is bound or assigned: every other use of the name sits right after
    /// `in`, where a loop calls it. Anything else (a second binding, an assignment, a
    /// parameter, passing it on) and it's just a local.
    fn find_aliases(&self, toks: &[Token]) -> Vec<(String, Iter)> {
        let mut out = Vec::new();
        for (d, t) in toks.iter().enumerate() {
            let Some(name) = t.name() else { continue };
            if is_keyword(name)
                || !(d > 0 && toks[d - 1].is_name("local"))
                || !toks.get(d + 1).is_some_and(|t| t.is("="))
            {
                continue;
            }
            let Some((iter, after)) = self.global_at(toks, d + 2) else {
                continue;
            };
            // The statement must end right there.
            let ends = match toks.get(after) {
                None => true,
                Some(t) if t.is(";") => true,
                Some(t) => t.name().is_some_and(|n| n != "and" && n != "or"),
            };
            if !ends {
                continue;
            }
            let only_in_loops = toks.iter().enumerate().all(|(j, u)| {
                j == d
                    || u.name() != Some(name)
                    || (j > 0 && (toks[j - 1].is(".") || toks[j - 1].is(":")))
                    || (j > 0 && toks[j - 1].is_name("in"))
            });
            if only_in_loops {
                out.push((name.to_owned(), iter));
            }
        }
        out
    }
}

/// `Name { .Name | [ ... ] | :Name( ... ) | ( ... ) }`: a variable, field, index or
/// call chain with no operators. Returns the index after it.
fn path(toks: &[Token], mut j: usize) -> Option<usize> {
    let first = toks.get(j)?.name()?;
    if is_keyword(first) {
        return None;
    }
    j += 1;
    loop {
        let Some(t) = toks.get(j) else { return Some(j) };
        if t.is(".") {
            toks.get(j + 1)?.name().filter(|n| !is_keyword(n))?;
            j += 2;
        } else if t.is("[") || t.is("(") {
            j = closing(toks, j)? + 1;
        } else if t.is(":") {
            toks.get(j + 1)?.name().filter(|n| !is_keyword(n))?;
            if !toks.get(j + 2)?.is("(") {
                return None;
            }
            j = closing(toks, j + 2)? + 1;
        } else {
            return Some(j);
        }
    }
}

/// `for ... in <iterator> do`: the iterator must be one whose order is the same on every
/// run. A bare `for k, v in t` walks the table in its internal order, which follows
/// heap addresses for keys that are tables or functions, and can't be intercepted at
/// runtime. So a generic `for` is accepted only in these exact forms, with nothing
/// else between `in` and `do`:
///
/// - `pairs(...)`, `ipairs(...)`, `string.gmatch(...)` or `utf8.codes(...)`: one call,
///   its parentheses balanced (the sandbox's `pairs` refuses tables keyed by tables or
///   functions; the others are ordered);
/// - `next, t` or `next, t, nil`, where `t` is a variable, field, index or call chain;
/// - the same through a local proven to alias one of them (`local gmatch = string.gmatch`).
///
/// Anything else is refused: an `and`/`or` expression, a second value, a shadowed
/// `pairs` or `string`, a parenthesised call.
fn unordered_iteration(
    toks: &[Token],
    at: usize,
    its: &Iterators,
    file: &str,
) -> Option<Diagnostic> {
    let ordered = || -> Option<bool> {
        let (names, j) = name_list(toks, at + 1);
        let after = toks.get(j)?;
        if names.len() == 1 && after.is("=") {
            return Some(true); // a numeric `for`
        }
        if names.is_empty() || !after.is_name("in") {
            return Some(false);
        }
        let (iter, k) = its.iter_at(toks, j + 1)?;
        let end = match iter {
            Iter::Next => {
                if !toks.get(k)?.is(",") {
                    return Some(false);
                }
                let mut e = path(toks, k + 1)?;
                if toks.get(e).is_some_and(|t| t.is(","))
                    && toks.get(e + 1).is_some_and(|t| t.is_name("nil"))
                {
                    e += 2;
                }
                e
            }
            _ => {
                if !toks.get(k)?.is("(") {
                    return Some(false);
                }
                closing(toks, k)? + 1
            }
        };
        Some(toks.get(end)?.is_name("do"))
    };
    (!ordered().unwrap_or(false)).then(|| {
        error(
            "unordered_iteration",
            "iterate with exactly one `pairs(t)`, `ipairs(t)`, `string.gmatch(...)` or \
             `utf8.codes(...)` call, or `next, t`, between `in` and `do`: a bare \
             `for k, v in t` (or an expression that may yield one) has no stable order \
             when keys are tables or functions",
            Some(file),
            Some(toks[at].line),
        )
    })
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
    let its = Iterators::new(&toks);
    let declared = &its.declared;
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
            if let Some(d) = unordered_iteration(&toks, i, &its, file) {
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
            || prev.is_some_and(|p| matches!(p.name(), Some("local" | "const" | "function")))
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

    /// The lines of every `unordered_iteration` in `src`.
    fn unordered(src: &str) -> Vec<u32> {
        scan("f.luau", src)
            .diagnostics
            .iter()
            .filter(|d| d.code == "unordered_iteration")
            .map(|d| d.location.as_ref().unwrap().line.unwrap())
            .collect()
    }

    #[test]
    fn loops_must_iterate_in_a_stable_order() {
        let ok = "for i = 1, 3 do end\nfor k, v in pairs(t) do end\nfor _, x in ipairs({ 1 }) do end\n\
             for k in next, t do end\nfor w in string.gmatch(s, \"%a+\") do end\n\
             for k: string, v: number in pairs(t) do end\nfor k, v in next, t, nil do end\n\
             for k in pairs(get(t, \"x\")) do end\nfor _, c in utf8.codes(s) do end\n\
             for i: number = 1, 3 do end\nfor k in next, a.b[c]:d(e) do end\n\
             for k in pairs --[[ note ]] (t) do end\n";
        assert!(unordered(ok).is_empty(), "{:?}", unordered(ok));
        let bad = "for k, v in t do end\nfor _, x in { 1, 2 } do end\nfor k in f() do end\n\
             local function pairs(x) return x end\nfor k in pairs(t) do end\n";
        assert_eq!(unordered(bad), vec![1, 2, 3, 5]);
    }

    #[test]
    fn an_iterator_must_be_the_whole_expression() {
        // The reviewer's bypass and its cousins: each could yield the table itself.
        let lines = [
            "for _, v in pairs({}) and t do end",
            "for w in string.gmatch and t do end",
            "for k in next and t do end",
            "for k in pairs(t) or t do end",
            "for k in pairs(t), u do end",
            "for k in pairs(t)(u) do end",
            "for k in (pairs(t)) do end",
            "for k in ipairs(t).x do end",
            "for k in next, t and u do end",
            "for k in next, t, k0 do end",
            "for k in next, (t) do end",
            "for k in utf8.codes do end",
            "for k in string.gmatch(s, p) and t do end",
            "for k in pairs(t do end",
        ];
        for line in lines {
            assert_eq!(unordered(line), vec![1], "{line}");
        }
    }

    #[test]
    fn shadowing_an_iterator_in_a_typed_list_is_seen() {
        // The type annotation hides a comma; `pairs` is still a local.
        let src = "local x: { a: number, b: number }, pairs = nil, function(t) return t end\n\
                   for k in pairs(t) do end\n";
        assert_eq!(unordered(src), vec![2]);
        let src = "local f: Map<string, number>, ipairs = nil, nil\nfor k in ipairs(t) do end\n";
        assert_eq!(unordered(src), vec![2]);
        let src = "local x: typeof(1 < 2), next = 1, 2\nfor k in next, t do end\n";
        assert_eq!(unordered(src), vec![2]);
        let src = "local function f(cb: (number, string) -> (), string)\n\
                   for w in string.gmatch(s, p) do end end\n";
        assert_eq!(unordered(src), vec![2]);
        // A type that only mentions `string` doesn't shadow it.
        let src = "local function f(cb: (string) -> (), s: string)\n\
                   for w in string.gmatch(s, \"%a+\") do end end\n";
        assert!(unordered(src).is_empty());
    }

    #[test]
    fn const_shadows_an_iterator_too() {
        // Luau's `const` binds a local just like `local` does.
        let src = "const function pairs<K, V>(x) return x end\nfor _, v in pairs(t) do end\n";
        assert_eq!(unordered(src), vec![2]);
        let src = "const string = { gmatch = next }\nfor w in string.gmatch(s, p) do end\n";
        assert_eq!(unordered(src), vec![2]);
    }

    #[test]
    fn aliases_must_be_proven() {
        let ok = "local gmatch = string.gmatch\nfor w in gmatch(s, \"%a+\") do end\n\
                  local nx = next\nfor k in nx, t do end\nlocal p = pairs;\nfor k in p(t) do end\n";
        assert!(unordered(ok).is_empty(), "{:?}", unordered(ok));
        let reassigned =
            "local g = string.gmatch\ng = function() return t end\nfor w in g(s) do end\n";
        assert_eq!(unordered(reassigned), vec![3]);
        let expression = "local g = pairs and t\nfor w in g(s) do end\n";
        assert_eq!(unordered(expression), vec![2]);
        let call = "local g = pairs\n(t)\nfor k in g(t) do end\n";
        assert_eq!(unordered(call), vec![3]);
        let rebound = "local g = pairs\nlocal function f(g) for k in g(t) do end end\n";
        assert_eq!(unordered(rebound), vec![2]);
        let passed = "local g = pairs\nlocal h = g\nfor k in g(t) do end\n";
        assert_eq!(unordered(passed), vec![3]);
    }

    #[test]
    fn strings_and_comments_cant_hide_a_loop() {
        // Each line holds a real bare loop that a naive tokenizer reads as text.
        let srcs = [
            // An interpolated string with a backtick inside its expression.
            "local s = `{\"`\"}` for _, v in t do end local z = \"x\"\n",
            // `\z` skips the line break, so the string goes on.
            "local s = \"a\\z\n  b\" for _, v in t do end\n",
            // A line comment ends at a lone `\r`.
            "-- note\rfor _, v in t do end\n",
            // `\u{...}` in an interpolated string isn't an expression.
            "local s = `\\u{41}` for _, v in t do end\n",
            // Nested interpolation.
            "local s = `a{`b{t}c`}d` for _, v in t do end\n",
            // A long string's closing level must match.
            "local s = [==[ ]] ]=] ]==] for _, v in t do end\n",
            // A backslash before a line break continues a quoted string.
            "local s = \"a\\\nb\" for _, v in t do end\n",
        ];
        for src in srcs {
            assert_eq!(unordered(src).len(), 1, "{src:?}");
        }
        // And text that only looks like a loop is text.
        let text = "local s = \"for _, v in t do end\" --[==[ for k in t do end ]==]\n\
                    local u = `for k in {\"t\"} do`\n-- for k in t do end\n";
        assert!(unordered(text).is_empty(), "{:?}", unordered(text));
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

//! Validation, in the research page's order: shape, references, rules, limits, text,
//! grounding.
//! Anything that fails is dropped; the host then uses the utility choice and the
//! authored line. Nothing is retried here.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::prelude::*;

use crate::menu::{Menu, MenuOption};
use crate::mind::{Decision, LineDraft, wire_index};

/// Why an answer was dropped.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Invalid {
    #[error("shape: {0}")]
    Shape(String),
    #[error("reference: {0}")]
    Reference(String),
    #[error("rules: {0}")]
    Rules(String),
    #[error("limits: {0}")]
    Limits(String),
    #[error("text: {0}")]
    Text(String),
}

/// 1. Shape: the answer is a choice, and the choice is on the menu.
pub fn choice(menu: &Menu, decision: &Decision) -> Result<usize, Invalid> {
    match decision {
        Decision::Choice { choice, .. } => wire_index(choice)
            .filter(|i| *i < menu.options.len())
            .ok_or_else(|| Invalid::Shape(format!("`{choice}` is not on the menu"))),
        other => Err(Invalid::Shape(format!("expected a choice, got {other:?}"))),
    }
}

/// Does an entity with this ID exist? Unknown ID kinds pass: only the stores the
/// world has can be checked.
pub fn exists(world: &WorldState, id: &str) -> bool {
    match id.split('.').next().unwrap_or_default() {
        "station" => world.stations.contains(id),
        "ware" => world.wares.contains(id),
        "ship" | "craft" => world.ships.contains(id),
        "npc" => world.npcs.contains(id),
        "faction" => world.factions.contains(id),
        "rumor" => world.rumors.contains(id),
        "region" => world.regions.contains(id),
        "lane" => world.lanes.contains(id),
        "rock" => world.rocks.contains(id),
        "field" => world.fields.contains(id),
        "hull" => world.hulls.contains(id),
        _ => true,
    }
}

/// 2 and 3. References exist and the menu's preconditions still hold. The world may
/// have moved on since the request.
pub fn option(world: &WorldState, menu: &Menu, option: &MenuOption) -> Result<(), Invalid> {
    if let Some(r) = option.refs.iter().find(|r| !exists(world, r.as_str())) {
        return Err(Invalid::Reference(format!("{r} does not exist")));
    }
    for p in &menu.preconditions {
        p.holds(world).map_err(Invalid::Rules)?;
    }
    Ok(())
}

/// What a line is checked against.
pub struct LineRules<'a> {
    /// The fact the line must cite.
    pub cite: FactId,
    /// Every fact in the packet, with its text. Nothing else may be cited.
    pub facts: &'a BTreeMap<FactId, String>,
    /// The packet's other texts (speaker, place, state). Names may come from here;
    /// numbers and claims may not.
    pub context: &'a [String],
    pub max_words: u32,
    pub max_chars: u32,
    /// Case-insensitive terms that may not appear.
    pub forbidden: &'a [String],
    /// Names of people, places, ships, factions and the like in the world (not the
    /// player's). Naming one needs grounding. A name of several words matches in any
    /// case; a one-word name matches when capitalised.
    pub names: &'a BTreeSet<String>,
    /// Ware names: naming one in any case needs grounding.
    pub wares: &'a BTreeSet<String>,
    /// Words that say something happened (`destroy`, `sold` ...). One may only be used
    /// if the authored line or a cited fact uses it.
    pub claims: &'a [String],
    /// The authored line, slots filled.
    pub authored: &'a str,
    /// The authored line with its slots left in.
    pub template: &'a str,
    /// Slot name -> value. The model must keep each slot as often as the template has
    /// it; the game fills it in.
    pub slots: &'a BTreeMap<String, String>,
}

/// A line that passed.
#[derive(Clone, Debug, PartialEq)]
pub struct CleanLine {
    pub text: String,
    pub cites: Vec<FactId>,
}

/// Strips a trailing `(fact.N)` the model may echo, surrounding quotes and extra
/// spaces.
fn clean(text: &str) -> String {
    fn unquote(t: &str) -> &str {
        let t = t.trim();
        t.strip_prefix(['"', '“'])
            .and_then(|x| x.strip_suffix(['"', '”']))
            .unwrap_or(t)
            .trim()
    }
    let mut t = unquote(text).to_owned();
    while let Some(open) = t.rfind("(fact.").filter(|_| t.ends_with(')')) {
        t = t[..open].trim_end().to_owned();
    }
    unquote(&t).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn english(c: char) -> bool {
    c.is_ascii_graphic()
        || c == ' '
        // Typographic punctuation and Latin-1 letters (café, naïve).
        || "‘’“”–—…".contains(c)
        || ('\u{C0}'..='\u{FF}').contains(&c)
}

/// Words in order, without surrounding punctuation or a possessive `'s`, lowercased.
fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
        .map(|w| w.trim_matches(['\'', '’']))
        .filter(|w| !w.is_empty())
        .map(|w| {
            w.trim_end_matches("'s")
                .trim_end_matches("’s")
                .to_lowercase()
        })
        .collect()
}

/// Does `phrase` occur in `hay` as whole words?
fn has_phrase(hay: &[String], phrase: &[String]) -> bool {
    !phrase.is_empty() && hay.windows(phrase.len()).any(|w| w == phrase)
}

fn word_number(w: &str) -> Option<u64> {
    const SMALL: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 8] = [
        "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    if let Some(i) = SMALL.iter().position(|x| *x == w) {
        return Some(i as u64);
    }
    if let Some(i) = TENS.iter().position(|x| *x == w) {
        return Some(20 + 10 * i as u64);
    }
    match w {
        "dozen" => Some(12),
        "hundred" => Some(100),
        "thousand" => Some(1_000),
        "million" => Some(1_000_000),
        _ => None,
    }
}

/// Every number a text states, in digits or words, normalised (`12`, `9.4`, `1200`).
/// A lone "one" is a pronoun more often than a count, so it only counts next to
/// another number word ("one hundred").
pub fn numbers(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut n = String::new();
            while i < chars.len() {
                let c = chars[i];
                let joins = matches!(c, '.' | ',')
                    && chars.get(i + 1).is_some_and(char::is_ascii_digit)
                    && !n.is_empty();
                if c.is_ascii_digit() {
                    n.push(c);
                } else if joins {
                    if c == '.' {
                        n.push('.');
                    }
                } else {
                    break;
                }
                i += 1;
            }
            if let Ok(v) = n.parse::<f64>() {
                out.insert(format!("{v}"));
            }
        } else {
            i += 1;
        }
    }
    let toks = tokens(s);
    let mut run: Vec<&str> = Vec::new();
    let mut flush = |run: &mut Vec<&str>| {
        if run.is_empty() || run.as_slice() == ["one"] {
            run.clear();
            return;
        }
        let (mut total, mut current) = (0u64, 0u64);
        for w in run.iter() {
            let v = word_number(w).expect("only number words");
            match *w {
                "dozen" => current = current.max(1) * 12,
                "hundred" => current = current.max(1) * 100,
                "thousand" | "million" => {
                    total += current.max(1) * v;
                    current = 0;
                }
                _ => current += v,
            }
        }
        out.insert((total + current).to_string());
        run.clear();
    };
    for t in &toks {
        if word_number(t).is_some() {
            run.push(t);
        } else {
            flush(&mut run);
        }
    }
    flush(&mut run);
    out
}

/// The claim word `token` is a form of, if any: `destroyed` -> `destroy`.
fn claim_of<'c>(token: &str, claims: &'c [String]) -> Option<&'c str> {
    let forms = [
        Some(token.to_owned()),
        token.strip_suffix('s').map(str::to_owned),
        token.strip_suffix("ed").map(str::to_owned),
        token.strip_suffix('d').map(str::to_owned),
        token.strip_suffix("ing").map(str::to_owned),
        token.strip_suffix("ing").map(|t| format!("{t}e")),
    ];
    claims
        .iter()
        .find(|c| forms.iter().flatten().any(|f| f == *c))
        .map(String::as_str)
}

/// Puts the slot values in. The model must keep each slot exactly as often as the
/// template does, and may not add slots of its own.
fn fill(text: &str, template: &str, slots: &BTreeMap<String, String>) -> Result<String, Invalid> {
    let mut out = text.to_owned();
    for (name, value) in slots {
        let slot = format!("{{{name}}}");
        let want = template.matches(&slot).count();
        let have = text.matches(&slot).count();
        if have != want {
            return Err(Invalid::Text(format!(
                "slot {slot} appears {have} times, not {want}"
            )));
        }
        out = out.replace(&slot, value);
    }
    if out.contains(['{', '}']) && !slots.is_empty() {
        return Err(Invalid::Text("a slot that isn't in the line".into()));
    }
    Ok(out)
}

/// 4 to 6. Limits, text and grounding: the slots kept and filled, length caps,
/// English, no forbidden terms, citations only from the packet (the line's own fact
/// included), and nothing the grounding doesn't say. Every number and claim word must
/// come from the authored line or a cited fact, and every name from those or the
/// packet. A line that names an entity or a quantity that isn't there is dropped for
/// the authored line: text is grounded or absent.
pub fn line(draft: &LineDraft, rules: &LineRules<'_>) -> Result<CleanLine, Invalid> {
    let text = clean(&draft.text);
    if text.is_empty() {
        return Err(Invalid::Text("empty".into()));
    }
    let cites: Vec<FactId> = draft
        .cites
        .iter()
        .map(|c| {
            c.parse::<FactId>()
                .map_err(|_| Invalid::Shape(format!("`{c}` is not a fact ID")))
        })
        .collect::<Result<_, _>>()?;
    if let Some(c) = cites.iter().find(|c| !rules.facts.contains_key(c)) {
        return Err(Invalid::Reference(format!("{c} is not in the packet")));
    }
    if !cites.contains(&rules.cite) {
        return Err(Invalid::Reference(format!(
            "the line must cite {}",
            rules.cite
        )));
    }
    let text = fill(&text, rules.template, rules.slots)?;
    let n_words = text.split_whitespace().count();
    if n_words > rules.max_words as usize {
        return Err(Invalid::Limits(format!(
            "{n_words} words, cap {}",
            rules.max_words
        )));
    }
    if text.chars().count() > rules.max_chars as usize {
        return Err(Invalid::Limits(format!(
            "longer than {} characters",
            rules.max_chars
        )));
    }
    if let Some(c) = text.chars().find(|c| !english(*c)) {
        return Err(Invalid::Text(format!("unexpected character {c:?}")));
    }
    if text.contains(['{', '}', '<', '>']) || text.to_lowercase().contains("fact.") {
        return Err(Invalid::Text("markup or IDs in the text".into()));
    }
    let lower = text.to_lowercase();
    if let Some(t) = rules
        .forbidden
        .iter()
        .find(|t| !t.is_empty() && lower.contains(&t.to_lowercase()))
    {
        return Err(Invalid::Text(format!("forbidden term `{t}`")));
    }
    grounded(&text, &cites, rules)?;
    Ok(CleanLine { text, cites })
}

/// 6. Grounding.
fn grounded(text: &str, cites: &[FactId], rules: &LineRules<'_>) -> Result<(), Invalid> {
    // What the line may state as fact: the authored line and the facts it cites.
    let mut stated = vec![rules.authored.to_owned()];
    stated.extend(cites.iter().filter_map(|c| rules.facts.get(c)).cloned());
    stated.extend(rules.slots.values().cloned());
    let stated = stated.join("\n");
    // What it may name: that, the other facts and the rest of the packet.
    let mut nameable = vec![stated.clone()];
    nameable.extend(rules.facts.values().cloned());
    nameable.extend(rules.context.iter().cloned());
    let nameable = nameable.join("\n");

    let have = numbers(&stated);
    if let Some(n) = numbers(text).into_iter().find(|n| !have.contains(n)) {
        return Err(Invalid::Text(format!(
            "the number {n} isn't in the line or its facts"
        )));
    }

    let line = tokens(text);
    let stated_claims: BTreeSet<&str> = tokens(&stated)
        .iter()
        .filter_map(|t| claim_of(t, rules.claims))
        .collect();
    if let Some(t) = line
        .iter()
        .find(|t| claim_of(t, rules.claims).is_some_and(|c| !stated_claims.contains(c)))
    {
        return Err(Invalid::Text(format!(
            "`{t}` isn't in the line or its facts"
        )));
    }

    let near = tokens(&nameable);
    let near_words: BTreeSet<&String> = near.iter().collect();
    for name in rules.wares.iter().chain(rules.names.iter()) {
        let phrase = tokens(name);
        let ware = rules.wares.contains(name);
        let named = if ware || phrase.len() > 1 {
            has_phrase(&line, &phrase)
        } else {
            // A one-word name counts when capitalised ("Wren", not "wren").
            text.split_whitespace().any(|w| {
                let w = w.trim_matches(|c: char| !c.is_alphanumeric());
                w.chars().next().is_some_and(char::is_uppercase) && tokens(w) == phrase
            })
        };
        if named && !has_phrase(&near, &phrase) {
            return Err(Invalid::Text(format!(
                "names {name}, which isn't in the packet"
            )));
        }
    }

    // No new named people, places or ships: a capitalised word must be grounded. At
    // the start of a sentence only words of a known name are checked.
    let name_words: BTreeSet<String> = rules
        .names
        .iter()
        .chain(rules.wares.iter())
        .flat_map(|n| tokens(n))
        .collect();
    let mut sentence_start = true;
    for raw in text.split_whitespace() {
        let w: String = raw
            .trim_matches(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
            .to_owned();
        let starts = sentence_start;
        sentence_start = raw.ends_with(['.', '!', '?', ':', '…', '"', '”']);
        if w.is_empty() || w == "I" || w.starts_with("I'") || w.starts_with("I’") {
            continue;
        }
        if !w.chars().next().is_some_and(char::is_uppercase) {
            continue;
        }
        let parts = tokens(&w);
        let checked = !starts || parts.iter().any(|p| name_words.contains(p));
        if checked && !parts.iter().all(|p| near_words.contains(p)) {
            return Err(Invalid::Text(format!("unknown name `{w}`")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        facts: BTreeMap<FactId, String>,
        context: Vec<String>,
        names: BTreeSet<String>,
        wares: BTreeSet<String>,
        claims: Vec<String>,
        forbidden: Vec<String>,
        slots: BTreeMap<String, String>,
    }

    fn fixture() -> Fixture {
        Fixture {
            facts: [
                (FactId(3), "Wren docked at Kettle Dock".to_owned()),
                (FactId(4), "Wren delivered 12 ice to Kettle Dock".to_owned()),
            ]
            .into(),
            context: vec!["Odo Fenn".into(), "Kettle Dock in Lantern Shallows".into()],
            names: [
                "Kettle Dock",
                "Odo Fenn",
                "Saffron Vey",
                "Brassmouth Exchange",
                "Wren",
                "Ninefold",
            ]
            .map(str::to_owned)
            .into(),
            wares: ["Ice", "Rations", "Iron ore"].map(str::to_owned).into(),
            claims: ["destroy", "sell", "sold", "deliver", "attack"]
                .map(str::to_owned)
                .into(),
            forbidden: vec!["Ancients".to_owned()],
            slots: BTreeMap::new(),
        }
    }

    impl Fixture {
        fn rules<'a>(&'a self, authored: &'a str, template: &'a str) -> LineRules<'a> {
            LineRules {
                cite: FactId(4),
                facts: &self.facts,
                context: &self.context,
                max_words: 12,
                max_chars: 100,
                forbidden: &self.forbidden,
                names: &self.names,
                wares: &self.wares,
                claims: &self.claims,
                authored,
                template,
                slots: &self.slots,
            }
        }
    }

    const ODO: &str = "Odo's been counting your ice.";

    fn draft(text: &str, cite: &str) -> LineDraft {
        LineDraft {
            text: text.into(),
            cites: vec![cite.into()],
        }
    }

    fn check(text: &str) -> Result<CleanLine, Invalid> {
        let f = fixture();
        line(&draft(text, "fact.4"), &f.rules(ODO, ODO))
    }

    #[test]
    fn accepts_a_good_line_and_cleans_it() {
        let ok = check("\"Odo’s tallying your 12 ice at Kettle Dock. (fact.4)\"").unwrap();
        assert_eq!(ok.text, "Odo’s tallying your 12 ice at Kettle Dock.");
        assert_eq!(ok.cites, vec![FactId(4)]);
        assert!(check("Twelve ice, and you delivered it. I saw.").is_ok());
        assert!(check("Odo counts. I do too.").is_ok());
    }

    #[test]
    fn rejects_missing_or_foreign_citations() {
        let f = fixture();
        let r = f.rules(ODO, ODO);
        assert!(matches!(
            line(&draft("Odo counts.", "fact.99"), &r),
            Err(Invalid::Reference(_))
        ));
        assert!(matches!(
            line(&draft("Odo counts.", "fact.3"), &r),
            Err(Invalid::Reference(_))
        ));
        assert!(matches!(
            line(&draft("Odo counts.", "nonsense"), &r),
            Err(Invalid::Shape(_))
        ));
    }

    #[test]
    fn rejects_long_foreign_forbidden_and_new_names() {
        let long = "one two three four five six seven eight nine ten eleven twelve thirteen";
        assert!(matches!(check(long), Err(Invalid::Limits(_))));
        assert!(matches!(check("Odo counts 氷."), Err(Invalid::Text(_))));
        assert!(matches!(check("The ancients hum."), Err(Invalid::Text(_))));
        assert!(matches!(
            check("Odo met Captain Varn today."),
            Err(Invalid::Text(_))
        ));
    }

    /// Review finding 1: a real citation must not launder an invented claim.
    #[test]
    fn a_valid_citation_does_not_launder_an_invented_claim() {
        for bad in [
            // The reviewer's case: wrong quantity and an event that never happened.
            "You delivered 900 ice and destroyed the station.",
            "You delivered nine hundred ice.",
            "You destroyed the station.",
            // Known entities the grounding doesn't mention.
            "Saffron Vey sends her thanks.",
            "Brassmouth Exchange wants your ice.",
            "Ninefold saw your ice come in.",
            "Bring rations next time.",
            "Some iron ore would help too.",
            // A number from nowhere, in words.
            "Thirty more and the beans bloom.",
            // An instruction smuggled in as a claim.
            "You sold the station to the Compact.",
        ] {
            assert!(
                matches!(check(bad), Err(Invalid::Text(_))),
                "accepted: {bad}"
            );
        }
    }

    #[test]
    fn numbers_read_digits_and_words() {
        let n = |s: &str| numbers(s).into_iter().collect::<Vec<_>>();
        assert_eq!(n("12 ice for 9.4 cr"), ["12", "9.4"]);
        assert_eq!(n("1,200 units"), ["1200"]);
        assert_eq!(n("twenty-two crates, a dozen more"), ["12", "22"]);
        assert_eq!(n("two hundred and one"), ["200"]);
        assert!(n("the old one").is_empty());
    }

    #[test]
    fn slots_must_be_kept_and_are_filled_by_the_game() {
        let mut f = fixture();
        f.slots = [("qty".to_owned(), "12".to_owned())].into();
        let template = "{qty} ice. The beans will hear about this.";
        let authored = "12 ice. The beans will hear about this.";
        let r = f.rules(authored, template);
        let ok = line(
            &draft("{qty} ice! The beans will hear of it.", "fact.4"),
            &r,
        )
        .unwrap();
        assert_eq!(ok.text, "12 ice! The beans will hear of it.");
        for bad in [
            "Twelve ice. The beans will hear of it.",
            "{qty} ice, {qty} more. The beans will hear.",
            "{qty} {ware}. The beans will hear.",
        ] {
            assert!(line(&draft(bad, "fact.4"), &r).is_err(), "accepted: {bad}");
        }
    }
}

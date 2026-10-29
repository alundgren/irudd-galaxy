//! Validation, in the research page's order: shape, references, rules, limits, text.
//! Anything that fails is dropped; the host then uses the utility choice and the
//! authored line. Nothing is retried here.

use std::collections::BTreeSet;

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
    /// Every fact in the packet.
    pub packet: &'a [FactId],
    pub max_words: u32,
    pub max_chars: u32,
    /// Case-insensitive terms that may not appear.
    pub forbidden: &'a [String],
    /// Names the line may use: the world's, plus the authored line's words.
    pub names: &'a BTreeSet<String>,
    pub authored: &'a str,
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

/// Words, without surrounding punctuation, lowercased.
fn words(s: &str) -> BTreeSet<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
        .filter(|w| !w.is_empty())
        .map(|w| {
            w.trim_end_matches("'s")
                .trim_end_matches("’s")
                .to_lowercase()
        })
        .collect()
}

/// 4 and 5. Limits and text: length caps, English, no forbidden terms, no names that
/// aren't already known, and citations only from the packet, including the line's own
/// fact.
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
    if let Some(c) = cites.iter().find(|c| !rules.packet.contains(c)) {
        return Err(Invalid::Reference(format!("{c} is not in the packet")));
    }
    if !cites.contains(&rules.cite) {
        return Err(Invalid::Reference(format!(
            "the line must cite {}",
            rules.cite
        )));
    }
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
    // No new named people, places or ships: a capitalised word that doesn't start a
    // sentence must be a known name or come from the authored line.
    let known: BTreeSet<String> = rules
        .names
        .iter()
        .flat_map(|n| words(n))
        .chain(words(rules.authored))
        .collect();
    let mut sentence_start = true;
    for raw in text.split_whitespace() {
        let w: String = raw
            .trim_matches(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
            .to_owned();
        let starts = sentence_start;
        sentence_start = raw.ends_with(['.', '!', '?', ':', '…', '"', '”']);
        if w.is_empty() || starts || w == "I" || w.starts_with("I'") || w.starts_with("I’") {
            continue;
        }
        if w.chars().next().is_some_and(char::is_uppercase)
            && !words(&w).iter().all(|x| known.contains(x))
        {
            return Err(Invalid::Text(format!("unknown name `{w}`")));
        }
    }
    Ok(CleanLine { text, cites })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules<'a>(names: &'a BTreeSet<String>, forbidden: &'a [String]) -> LineRules<'a> {
        LineRules {
            cite: FactId(4),
            packet: &[FactId(3), FactId(4)],
            max_words: 12,
            max_chars: 100,
            forbidden,
            names,
            authored: "Odo's been counting your ice.",
        }
    }

    fn draft(text: &str, cite: &str) -> LineDraft {
        LineDraft {
            text: text.into(),
            cites: vec![cite.into()],
        }
    }

    #[test]
    fn accepts_a_good_line_and_cleans_it() {
        let names: BTreeSet<String> = ["Kettle Dock".to_owned()].into();
        let ok = line(
            &draft(
                "\"Odo’s tallying your ice at Kettle Dock. (fact.4)\"",
                "fact.4",
            ),
            &rules(&names, &[]),
        )
        .unwrap();
        assert_eq!(ok.text, "Odo’s tallying your ice at Kettle Dock.");
        assert_eq!(ok.cites, vec![FactId(4)]);
    }

    #[test]
    fn rejects_missing_or_foreign_citations() {
        let names = BTreeSet::new();
        let r = rules(&names, &[]);
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
        let names = BTreeSet::new();
        let forbidden = vec!["Ancients".to_owned()];
        let r = rules(&names, &forbidden);
        let long = "one two three four five six seven eight nine ten eleven twelve thirteen";
        assert!(matches!(
            line(&draft(long, "fact.4"), &r),
            Err(Invalid::Limits(_))
        ));
        assert!(matches!(
            line(&draft("Odo counts 氷.", "fact.4"), &r),
            Err(Invalid::Text(_))
        ));
        assert!(matches!(
            line(&draft("The ancients hum.", "fact.4"), &r),
            Err(Invalid::Text(_))
        ));
        assert!(matches!(
            line(&draft("Odo met Captain Varn today.", "fact.4"), &r),
            Err(Invalid::Text(_))
        ));
        assert!(line(&draft("Odo counts. I do too.", "fact.4"), &r).is_ok());
    }
}

//! Module-namespaced IDs.
//!
//! Everything a module names (component types, event kinds, entities) has a full ID
//! `<module>.<name>`, such as `example.door`. Inside its own files a module may write the
//! local name (`door`); everywhere else the full ID is used. The engine's own names live in
//! the namespace [`ENGINE`] (`engine.position`), which no module may take.
//!
//! - A module ID starts with a lowercase letter and uses lowercase letters, digits, `-`
//!   and `_`.
//! - Component types, event kinds and fields have one-part names: lowercase letters,
//!   digits and `_`, starting with a letter (`door`, `push_count`).
//! - An entity's local name may have several dot-separated parts: `door`, `row.03`.

/// The engine's namespace.
pub const ENGINE: &str = "engine";

/// Longest accepted full ID, in bytes.
pub const MAX_ID_LEN: usize = 128;

pub fn valid_module_id(id: &str) -> bool {
    id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn segment(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A local name: `door`, `row.03`.
pub fn valid_local(name: &str) -> bool {
    name.len() <= MAX_ID_LEN
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.split('.').all(segment)
}

/// A field name: one segment starting with a letter.
pub fn valid_field(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase()) && segment(name)
}

/// Why `name` is not a one-part name.
pub fn name_rule(name: &str) -> String {
    format!(
        "`{name}` is not a valid name: start with a lowercase letter and use only lowercase \
         letters, digits and `_`"
    )
}

/// Why `name` is not a local name.
pub fn local_rule(name: &str) -> String {
    format!(
        "`{name}` is not a valid name: start with a lowercase letter and use lowercase \
         letters, digits and `_`, with `.` between parts"
    )
}

/// `module.name` into its module and local name.
pub fn split(full: &str) -> Option<(&str, &str)> {
    let (module, local) = full.split_once('.')?;
    (full.len() <= MAX_ID_LEN && valid_module_id(module) && valid_local(local))
        .then_some((module, local))
}

pub fn is_full_id(s: &str) -> bool {
    split(s).is_some()
}

/// The full ID of a component type or event kind written in module `module`: a name
/// with a dot is already full (`engine.position`), a plain one is the module's own.
pub fn qualify_type(module: &str, name: &str) -> Option<String> {
    if name.contains('.') {
        return split(name).map(|_| name.to_owned());
    }
    valid_field(name).then(|| format!("{module}.{name}"))
}

/// The full ID of an entity written in module `module`: `module.x` as it is, anything else
/// as a local name in the module's namespace.
pub fn qualify_entity(module: &str, name: &str) -> Option<String> {
    if let Some((ns, _)) = split(name)
        && ns == module
    {
        return Some(name.to_owned());
    }
    (valid_local(name) && module.len() + 1 + name.len() <= MAX_ID_LEN)
        .then(|| format!("{module}.{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_qualifies() {
        assert_eq!(split("example.door"), Some(("example", "door")));
        assert_eq!(split("my-mod.row.03"), Some(("my-mod", "row.03")));
        assert_eq!(split("door"), None);
        assert_eq!(split("Example.door"), None);
        assert_eq!(split("example.3door"), None);
        let ty = |n: &str| qualify_type("example", n);
        assert_eq!(ty("door").as_deref(), Some("example.door"));
        assert_eq!(ty("other.door").as_deref(), Some("other.door"));
        assert_eq!(ty("engine.position").as_deref(), Some("engine.position"));
        assert_eq!(ty("Bad"), None);
        assert_eq!(ty("bad."), None);
        let entity = |n: &str| qualify_entity("example", n);
        assert_eq!(entity("row.a").as_deref(), Some("example.row.a"));
        assert_eq!(entity("example.row").as_deref(), Some("example.row"));
        assert_eq!(entity("other.x").as_deref(), Some("example.other.x"));
    }
}

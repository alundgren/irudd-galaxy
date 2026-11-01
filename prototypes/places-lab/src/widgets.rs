//! Widgets built in Rust, in the screens' own vocabulary (`lab/prelude.luau`), for
//! variants that add to the screen: a talk variant's bubble or portrait, a life
//! variant's "what changed" note. They go through the same reading, layout and rules as
//! the Luau screens.
//!
//! ```ignore
//! W::new("panel").set("role", "speech").set("place", "bottom")
//!     .child(W::text("Ilse Marrow").set("style", "heading"))
//! ```

use mlua::{Lua, LuaSerdeExt, Table};
use serde_json::Value as Json;

#[derive(Clone, Debug)]
pub struct W {
    kind: &'static str,
    text: Option<String>,
    props: Vec<(String, Json)>,
    children: Vec<W>,
}

#[allow(dead_code)] // `text` and `child` are for variants
impl W {
    pub fn new(kind: &'static str) -> W {
        W {
            kind,
            text: None,
            props: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn text(s: impl Into<String>) -> W {
        let mut w = W::new("text");
        w.text = Some(s.into());
        w
    }

    /// A property: `role`, `place`, `anchor`, `on`, `args`, ...
    pub fn set(mut self, k: &str, v: impl Into<Json>) -> W {
        self.props.push((k.to_string(), v.into()));
        self
    }

    pub fn child(mut self, c: W) -> W {
        self.children.push(c);
        self
    }

    pub fn to_lua(&self, lua: &Lua) -> mlua::Result<Table> {
        let t = lua.create_table()?;
        t.raw_set("kind", self.kind)?;
        let mut i = 1;
        if let Some(s) = &self.text {
            t.raw_set(i, s.as_str())?;
            i += 1;
        }
        for c in &self.children {
            t.raw_set(i, c.to_lua(lua)?)?;
            i += 1;
        }
        for (k, v) in &self.props {
            if !v.is_null() {
                t.raw_set(k.as_str(), lua.to_value(v)?)?;
            }
        }
        Ok(t)
    }
}

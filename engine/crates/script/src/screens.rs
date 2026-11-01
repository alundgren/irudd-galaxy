//! Pure screen calls. Tracked component reads invalidate cached trees; no tick, writes,
//! random stream or persistent VM state is exposed to presentation code.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use galaxy_api::command::Fields;
use galaxy_api::{Registry, WorldView};
use galaxy_module::{ids, module::Module};
use mlua::{LuaSerdeExt, Table, Value};

use crate::vm::{Code, LIMITS, Vm};

pub const KINDS: [&str; 6] = ["text", "button", "list", "panel", "row", "column"];

#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub kind: String,
    pub id: Option<String>,
    pub path: String,
    pub text: String,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub size: f32,
    pub pad: f32,
    pub gap: f32,
    pub anchor: Option<String>,
    pub on: Option<String>,
    pub payload: Fields,
    pub children: Vec<Widget>,
}

#[derive(Default)]
struct Reads {
    fields: BTreeMap<(String, String), Option<Fields>>,
    queries: BTreeMap<String, Vec<String>>,
}
impl Reads {
    fn changed(&self, world: &dyn WorldView) -> bool {
        self.fields
            .iter()
            .any(|((e, c), f)| world.get(e, c) != f.as_ref())
            || self.queries.iter().any(|(c, ids)| {
                world.with_component(c) != ids.iter().map(String::as_str).collect::<Vec<_>>()
            })
    }
}

/// One screen function, kept alongside the view, outside the simulation.
pub struct Screen {
    module: Module,
    file: String,
    registry: Registry,
    code: Code,
    reads: Reads,
    initialized: bool,
    pub tree: Option<Widget>,
    pub problems: Vec<String>,
    pub calls: u64,
}
impl Screen {
    pub fn new(module: &Module, file: &str, registry: &Registry) -> Self {
        Self {
            module: module.clone(),
            file: file.into(),
            registry: registry.clone(),
            code: Code::default(),
            reads: Reads::default(),
            initialized: false,
            tree: None,
            problems: Vec::new(),
            calls: 0,
        }
    }

    /// Rust checks dependencies each frame. Luau runs only on the first call or changed reads.
    pub fn refresh(&mut self, world: &dyn WorldView) {
        if self.initialized && !self.reads.changed(world) {
            return;
        }
        self.initialized = true;
        self.calls += 1;
        let reads = RefCell::new(Reads::default());
        let path = format!("{}:{}/screen", self.module.id(), self.file);
        let result = self.call(world, &reads, &path);
        self.reads = reads.into_inner();
        self.problems.clear();
        match result {
            Ok(tree) => self.tree = tree,
            Err(e) => {
                self.tree = None;
                self.problems.push(format!("{path}: {e}"));
            }
        }
    }

    fn call(
        &self,
        world: &dyn WorldView,
        reads: &RefCell<Reads>,
        path: &str,
    ) -> Result<Option<Widget>, String> {
        let vm = Vm::new(Rc::new(self.module.files.clone()), &self.code, LIMITS)
            .map_err(|e| e.to_string())?;
        let lua = &vm.lua;
        vm.budgeted(|| {
            lua.scope(|scope| {
                let ctx = lua.create_table()?;
                ctx.set("module", self.module.id())?;
                ctx.set(
                    "get",
                    scope.create_function(|lua, (e, c): (String, String)| {
                        let e = entity_id(self.module.id(), &e, &self.registry)
                            .ok_or_else(|| mlua::Error::runtime("invalid entity ID"))?;
                        let c = ids::qualify_type(self.module.id(), &c)
                            .ok_or_else(|| mlua::Error::runtime("invalid component ID"))?;
                        let found = world.get(&e, &c);
                        vm.budget.charge(
                            lua,
                            1 + found
                                .map_or(0, |f| f.len() as u64 / crate::meter::RECORDS_PER_STEP),
                        )?;
                        let fields = found.cloned();
                        reads.borrow_mut().fields.insert((e, c), fields.clone());
                        match fields {
                            None => Ok(Value::Nil),
                            Some(fields) => {
                                Ok(Value::Table(crate::host::object_to_lua(lua, &fields)?))
                            }
                        }
                    })?,
                )?;
                ctx.set(
                    "query",
                    scope.create_function(|lua, c: String| {
                        let c = ids::qualify_type(self.module.id(), &c)
                            .ok_or_else(|| mlua::Error::runtime("invalid component ID"))?;
                        let found: Vec<String> = world
                            .with_component(&c)
                            .into_iter()
                            .map(str::to_owned)
                            .collect();
                        vm.budget
                            .charge(lua, 1 + found.len() as u64 / crate::meter::RECORDS_PER_STEP)?;
                        reads.borrow_mut().queries.insert(c, found.clone());
                        lua.create_sequence_from(found)
                    })?,
                )?;
                ctx.set_readonly(true);
                let ui = lua.create_table()?;
                for kind in KINDS {
                    ui.set(
                        kind,
                        lua.create_function(move |_, t: Table| {
                            t.raw_set("kind", kind)?;
                            Ok(t)
                        })?,
                    )?;
                }
                ui.set_readonly(true);
                let Value::Table(def) = vm.run_file(&self.file)? else {
                    return Err(mlua::Error::runtime("script must return a table"));
                };
                let f: mlua::Function = def.get("screen")?;
                let result: Value = f.call((ctx, ui))?;
                if result.is_nil() {
                    return Ok(None);
                }
                let mut count = 0;
                read(
                    lua,
                    result,
                    path,
                    self.module.id(),
                    &self.registry,
                    0,
                    &mut count,
                )
                .map(Some)
            })
        })
        .map_err(|e| e.to_string())
    }
}

fn read(
    lua: &mlua::Lua,
    value: Value,
    path: &str,
    module: &str,
    registry: &Registry,
    depth: usize,
    count: &mut usize,
) -> mlua::Result<Widget> {
    let bad = |s: &str| mlua::Error::runtime(format!("{path}: {s}"));
    *count += 1;
    if depth > 32 || *count > 512 {
        return Err(bad("screen exceeds 32 levels or 512 widgets"));
    }
    let Value::Table(t) = value else {
        return Err(bad("expected a widget table"));
    };
    let kind: String = t
        .get("kind")
        .map_err(|_| bad("use ui.text/button/list/panel/row/column"))?;
    if !KINDS.contains(&kind.as_str()) {
        return Err(bad("unknown widget kind"));
    }
    for pair in t.clone().pairs::<String, Value>() {
        let (key, _) =
            pair.map_err(|_| bad("properties must have names; put widgets in children"))?;
        if ![
            "kind", "id", "text", "width", "height", "x", "y", "size", "pad", "gap", "anchor",
            "on", "payload", "children",
        ]
        .contains(&key.as_str())
        {
            return Err(bad(&format!("unknown property `{key}`")));
        }
    }
    let num = |key: &str| -> mlua::Result<Option<f32>> {
        let n = match t.get::<Value>(key)? {
            Value::Nil => None,
            Value::Integer(n) => Some(n as f32),
            Value::Number(n) => Some(n as f32),
            _ => return Err(bad(&format!("{key} must be a number"))),
        };
        if n.is_some_and(|n| {
            !n.is_finite() || n.abs() > 100_000.0 || (key != "x" && key != "y" && n < 0.0)
        }) {
            return Err(bad(&format!("{key} must be a finite, usable dimension")));
        }
        Ok(n)
    };
    let string = |key: &str| string_field(&t, key).map_err(|e| bad(&e.to_string()));
    let id = string("id")?;
    let on = string("on")?;
    let on = on
        .map(|on| {
            ids::qualify_type(module, &on)
                .filter(|id| registry.events.contains(id) && id.starts_with(&format!("{module}.")))
                .ok_or_else(|| bad("on must name one of this module's declared events"))
        })
        .transpose()?;
    if kind == "button" && on.is_none() {
        return Err(bad("button needs an on event"));
    }
    if kind != "button" && on.is_some() {
        return Err(bad("only buttons have an on event"));
    }
    let payload: Fields = match t.get::<Value>("payload")? {
        Value::Nil => Fields::new(),
        v => lua
            .from_value(v)
            .map_err(|_| bad("payload must be a table of JSON fields"))?,
    };
    if let Some(on) = &on {
        registry
            .check(
                galaxy_api::Actor::Engine,
                &galaxy_api::Command::Record {
                    kind: on.clone(),
                    payload: payload.clone(),
                },
            )
            .map_err(|e| bad(&e))?;
    }
    let mut children = Vec::new();
    let list = match t.get::<Value>("children")? {
        Value::Nil => None,
        Value::Table(list) => Some(list),
        _ => return Err(bad("children must be a list of widgets")),
    };
    if let Some(list) = list {
        let len = list.raw_len();
        let mut entries = 0;
        for pair in list.clone().pairs::<Value, Value>() {
            let (key, _) = pair?;
            if !matches!(key, Value::Integer(n) if n >= 1 && (n as usize) <= len) {
                return Err(bad(
                    "children must be a list of widgets with consecutive indices starting at 1; use children = { ui.text {...}, ... }",
                ));
            }
            entries += 1;
        }
        if entries != len {
            return Err(bad("children must be a list of widgets without gaps"));
        }
        for (i, child) in list.sequence_values::<Value>().enumerate() {
            let child = child?;
            let label = if let Value::Table(t) = &child {
                string_field(t, "id")
                    .map_err(|e| bad(&format!("children[{}]: {e}", i + 1)))?
                    .or(t.get::<Option<String>>("kind")?)
                    .unwrap_or_else(|| "widget".into())
            } else {
                "widget".into()
            };
            children.push(read(
                lua,
                child,
                &format!("{path}/{label}[{}]", i + 1),
                module,
                registry,
                depth + 1,
                count,
            )?);
        }
    }
    if (kind == "text" || kind == "button") && !children.is_empty() {
        return Err(bad("text and button have no children"));
    }
    let anchor = string("anchor")?;
    let anchor = anchor
        .map(|a| {
            let (entity, name) = a
                .split_once('#')
                .ok_or_else(|| bad("anchor is publisher#name"))?;
            let entity = entity_id(module, entity, registry)
                .ok_or_else(|| bad("invalid anchor publisher"))?;
            Ok::<_, mlua::Error>(format!("{entity}#{name}"))
        })
        .transpose()?;
    Ok(Widget {
        kind: kind.clone(),
        id,
        path: path.into(),
        text: string("text")?.unwrap_or_default(),
        width: num("width")?,
        height: num("height")?,
        x: num("x")?,
        y: num("y")?,
        size: num("size")?.unwrap_or(20.0),
        pad: num("pad")?.unwrap_or(if kind == "panel" || kind == "button" {
            16.0
        } else {
            0.0
        }),
        gap: num("gap")?.unwrap_or(12.0),
        anchor,
        on,
        payload,
        children,
    })
}

fn string_field(t: &Table, key: &str) -> mlua::Result<Option<String>> {
    match t.get::<Value>(key)? {
        Value::Nil => Ok(None),
        Value::String(s) => s.to_str().map(|s| Some(s.to_string())),
        _ => Err(mlua::Error::runtime(format!("{key} must be a string"))),
    }
}

fn entity_id(module: &str, name: &str, registry: &Registry) -> Option<String> {
    if let Some((ns, _)) = ids::split(name)
        && (ns == "engine" || registry.modules.contains_key(ns))
    {
        return Some(name.into());
    }
    ids::qualify_entity(module, name)
}

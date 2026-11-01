//! The Luau side: `lab/main.luau` returns `{ init, on, view }`.
//!
//! - `init()` makes the state table.
//! - `on[name](state, args)` handles an event (a click, a drop) and changes the state.
//! - `view(state, ui)` returns the screen's widget tree.
//!
//! The view runs only after an event or a reload, never per frame. Every call has a time
//! budget, so a runaway loop stops with an error instead of freezing the window. Files
//! reload when they change; the state survives a reload.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use mlua::{Function, Lua, LuaSerdeExt, RegistryKey, Table, Value, VmState};
use serde_json::Value as Json;

/// How long one call may run.
const BUDGET: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub view_runs: u64,
    pub event_runs: u64,
    pub last_view_ms: f64,
    pub max_view_ms: f64,
    pub last_event_ms: f64,
}

pub struct Host {
    lua: Lua,
    dir: PathBuf,
    lab: Option<RegistryKey>,
    state: Option<RegistryKey>,
    ui: Option<RegistryKey>,
    used: Rc<RefCell<HashMap<String, RegistryKey>>>,
    deadline: Rc<Cell<Option<Instant>>>,
    stamps: Vec<(PathBuf, Option<SystemTime>)>,
    last_poll: Instant,
    pub stats: Stats,
    /// The last load or run error, shown on screen.
    pub error: Option<String>,
}

fn stamps(dir: &Path) -> Vec<(PathBuf, Option<SystemTime>)> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "luau"))
        .map(|p| {
            let t = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
            (p, t)
        })
        .collect();
    out.sort();
    out
}

fn short(e: &mlua::Error) -> String {
    let s = e.to_string();
    s.lines().take(6).collect::<Vec<_>>().join("\n")
}

impl Host {
    pub fn new(dir: PathBuf) -> Host {
        let lua = Lua::new();
        let deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
        let d = deadline.clone();
        lua.set_interrupt(move |_| {
            if let Some(t) = d.get()
                && Instant::now() > t
            {
                return Err(mlua::Error::runtime(format!(
                    "the script ran for more than {} ms and was stopped",
                    BUDGET.as_millis()
                )));
            }
            Ok(VmState::Continue)
        });
        let mut host = Host {
            lua,
            stamps: stamps(&dir),
            dir,
            lab: None,
            state: None,
            ui: None,
            used: Rc::new(RefCell::new(HashMap::new())),
            deadline,
            last_poll: Instant::now(),
            stats: Stats::default(),
            error: None,
        };
        host.load();
        host
    }

    fn arm(&self) {
        self.deadline.set(Some(Instant::now() + BUDGET));
    }

    fn disarm(&self) {
        self.deadline.set(None);
    }

    /// (Re)loads the prelude and `main.luau`. Keeps the state unless there is none.
    fn load(&mut self) {
        self.used.borrow_mut().clear();
        match self.try_load() {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("loading the lab: {}", short(&e))),
        }
    }

    fn try_load(&mut self) -> mlua::Result<()> {
        let lua = &self.lua;
        // `use(name)` loads `lab/<name>.luau` once per reload.
        let dir = self.dir.clone();
        let used = self.used.clone();
        let use_fn = lua.create_function(move |lua, name: String| {
            if !name.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                return Err(mlua::Error::runtime(format!(
                    "use({name:?}): names are lower_case"
                )));
            }
            if let Some(k) = used.borrow().get(&name) {
                return lua.registry_value::<Value>(k);
            }
            let path = dir.join(format!("{name}.luau"));
            let src = std::fs::read_to_string(&path)
                .map_err(|e| mlua::Error::runtime(format!("use({name:?}): {e}")))?;
            let v: Value = lua.load(&src).set_name(format!("@{name}.luau")).eval()?;
            let key = lua.create_registry_value(v.clone())?;
            used.borrow_mut().insert(name, key);
            Ok(v)
        })?;
        lua.globals().set("use", use_fn)?;
        self.arm();
        let result: mlua::Result<(Table, Table, Table)> = (|| {
            let prelude = std::fs::read_to_string(self.dir.join("prelude.luau"))
                .map_err(|e| mlua::Error::runtime(format!("prelude.luau: {e}")))?;
            let ui: Table = lua.load(&prelude).set_name("@prelude.luau").eval()?;
            lua.globals().set("ui", ui.clone())?;
            let main = std::fs::read_to_string(self.dir.join("main.luau"))
                .map_err(|e| mlua::Error::runtime(format!("main.luau: {e}")))?;
            let lab: Table = lua.load(&main).set_name("@main.luau").eval()?;
            let _: Function = lab.get("view")?;
            let _: Table = lab.get("on")?;
            let state = match &self.state {
                Some(k) => lua.registry_value::<Table>(k)?,
                None => {
                    let init: Function = lab.get("init")?;
                    init.call::<Table>(())?
                }
            };
            Ok((ui, lab, state))
        })();
        self.disarm();
        let (ui, lab, state) = result?;
        self.ui = Some(lua.create_registry_value(ui)?);
        self.lab = Some(lua.create_registry_value(lab)?);
        self.state = Some(lua.create_registry_value(state)?);
        Ok(())
    }

    /// Starts again from `init()`.
    pub fn reset(&mut self) {
        self.state = None;
        self.load();
    }

    /// Reloads if a file changed (checked at most twice a second). True if it did.
    pub fn poll_reload(&mut self) -> bool {
        if self.last_poll.elapsed() < Duration::from_millis(500) {
            return false;
        }
        self.last_poll = Instant::now();
        let now = stamps(&self.dir);
        if now == self.stamps {
            return false;
        }
        self.stamps = now;
        self.load();
        true
    }

    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    /// Runs `view(state, ui)`.
    pub fn view(&mut self) -> Option<Value> {
        let (lab, state, ui) = (self.lab.as_ref()?, self.state.as_ref()?, self.ui.as_ref()?);
        let t = Instant::now();
        self.arm();
        let result = (|| {
            let lab: Table = self.lua.registry_value(lab)?;
            let view: Function = lab.get("view")?;
            let state: Table = self.lua.registry_value(state)?;
            let ui: Table = self.lua.registry_value(ui)?;
            view.call::<Value>((state, ui))
        })();
        self.disarm();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        self.stats.view_runs += 1;
        self.stats.last_view_ms = ms;
        self.stats.max_view_ms = self.stats.max_view_ms.max(ms);
        match result {
            Ok(v) => Some(v),
            Err(e) => {
                self.error = Some(format!("view: {}", short(&e)));
                None
            }
        }
    }

    /// Sends an event to `on[name]`. A missing handler is an error the lab shows.
    pub fn event(&mut self, name: &str, args: &Json) {
        let (Some(lab), Some(state)) = (self.lab.as_ref(), self.state.as_ref()) else {
            return;
        };
        let t = Instant::now();
        self.arm();
        let result = (|| {
            let lab: Table = self.lua.registry_value(lab)?;
            let on: Table = lab.get("on")?;
            let f: Option<Function> = on.get(name)?;
            let Some(f) = f else {
                return Err(mlua::Error::runtime(format!("no handler on.{name}")));
            };
            let state: Table = self.lua.registry_value(state)?;
            let args = if args.is_null() {
                Value::Table(self.lua.create_table()?)
            } else {
                self.lua.to_value(args)?
            };
            f.call::<()>((state, args))
        })();
        self.disarm();
        self.stats.event_runs += 1;
        self.stats.last_event_ms = t.elapsed().as_secs_f64() * 1000.0;
        match result {
            Ok(()) => {
                if self
                    .error
                    .as_deref()
                    .is_some_and(|e| e.starts_with("event"))
                {
                    self.error = None;
                }
            }
            Err(e) => self.error = Some(format!("event {name}: {}", short(&e))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_file_reloads_and_keeps_the_state() {
        let dir = std::env::temp_dir().join(format!("ui-lab-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lab = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lab");
        std::fs::copy(lab.join("prelude.luau"), dir.join("prelude.luau")).unwrap();
        let main = |label: &str| {
            format!(
                "local lab = {{ on = {{}} }}\n\
                 function lab.init() return {{ n = 0 }} end\n\
                 function lab.on.add(state) state.n += 1 end\n\
                 function lab.view(state, ui) return ui.screen({{ ui.text({{ `{label} {{state.n}}`, place = \"top\" }}) }}) end\n\
                 return lab\n"
            )
        };
        std::fs::write(dir.join("main.luau"), main("before")).unwrap();
        let mut host = Host::new(dir.clone());
        host.event("add", &Json::Null);
        let text = |host: &mut Host| -> String {
            let v = host.view().expect("view runs");
            let Value::Table(t) = v else {
                panic!("a table")
            };
            let child: Table = t.get(1).unwrap();
            child.get::<String>(1).unwrap()
        };
        assert_eq!(text(&mut host), "before 1");
        // Make sure the file's time moves on, then change it.
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(dir.join("main.luau"), main("after")).unwrap();
        host.last_poll = Instant::now() - Duration::from_secs(1);
        assert!(host.poll_reload());
        assert_eq!(host.error, None);
        assert_eq!(text(&mut host), "after 1", "the state survives the reload");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_runaway_script_is_stopped() {
        let dir = std::env::temp_dir().join(format!("ui-lab-runaway-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lab = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lab");
        std::fs::copy(lab.join("prelude.luau"), dir.join("prelude.luau")).unwrap();
        std::fs::write(
            dir.join("main.luau"),
            "local lab = { on = {} }\nfunction lab.init() return {} end\n\
             function lab.on.spin() while true do end end\n\
             function lab.view(state, ui) return ui.screen({}) end\nreturn lab\n",
        )
        .unwrap();
        let mut host = Host::new(dir.clone());
        host.event("spin", &Json::Null);
        assert!(
            host.error.as_deref().unwrap_or("").contains("was stopped"),
            "{:?}",
            host.error
        );
        assert!(host.view().is_some(), "the lab carries on");
        std::fs::remove_dir_all(&dir).ok();
    }
}

//! The host on small modules made in the test.

use std::collections::BTreeMap;
use std::path::Path;

use galaxy_api::command::{Command, Fields};
use galaxy_api::rng::SimRng;
use galaxy_api::{Registry, WorldView};
use galaxy_module::module::{Module, ModuleFiles};
use serde_json::json;

use crate::host::{Event, ScriptSet, Target};

/// A world with a few components, for reads.
#[derive(Default)]
struct Store(BTreeMap<(String, String), Fields>);

impl WorldView for Store {
    fn tick(&self) -> u64 {
        7
    }
    fn get(&self, entity: &str, component: &str) -> Option<&Fields> {
        self.0.get(&(entity.to_owned(), component.to_owned()))
    }
    fn exists(&self, entity: &str) -> bool {
        self.0.keys().any(|(e, _)| e == entity)
    }
    fn with_component(&self, component: &str) -> Vec<&str> {
        self.0
            .keys()
            .filter(|(_, c)| c == component)
            .map(|(e, _)| e.as_str())
            .collect()
    }
}

const MANIFEST: &str = "id = \"aa\"\nversion = \"0.1.0\"\nmin_engine_version = \"0.1.0\"\n\
                        scripts = [\"main.luau\"]\nevents = [\"rang\"]\n\
                        writes = [\"engine.position\"]\n\
                        [components.bell]\nrings = \"int\"\nnote = \"text\"\n";

fn module(script: &str) -> Module {
    let mut files = ModuleFiles::default();
    files.0.insert("manifest.toml".into(), MANIFEST.into());
    files.0.insert("main.luau".into(), script.into());
    files.0.insert(
        "lib.luau".into(),
        "return { twice = function(n) return n * 2 end }".into(),
    );
    Module::from_files(Path::new("aa"), files).unwrap()
}

fn store() -> Store {
    let mut s = Store::default();
    s.0.insert(
        ("aa.one".into(), "aa.bell".into()),
        json!({ "rings": 2, "note": "a" })
            .as_object()
            .unwrap()
            .clone(),
    );
    s
}

fn run(script: &str, target: Target<'_>) -> crate::Outcome {
    let m = module(script);
    let registry = Registry::new(&[&m]).unwrap();
    let set = ScriptSet::load(&m, &registry).unwrap();
    set.call(0, target, &store(), &registry, &mut SimRng::for_key(1, "t"))
}

fn cadence(script: &str) -> crate::Outcome {
    run(script, Target::Cadence)
}

#[test]
fn ctx_binds_exactly_the_declared_api() {
    let out = cadence(
        "return { cadence = function(ctx)
            local names = {}
            for k in pairs(ctx) do table.insert(names, k) end
            table.sort(names)
            ctx.record('rang', { names = table.concat(names, ',') })
        end }",
    );
    let commands = out.result.unwrap();
    let Command::Record { payload, .. } = &commands[0] else {
        panic!("{commands:?}")
    };
    let mut declared: Vec<&str> = crate::api::CTX.iter().map(|m| m.name).collect();
    declared.sort();
    assert_eq!(payload["names"], declared.join(","));
}

#[test]
fn handlers_read_and_write_through_ctx() {
    let script = "local lib = require('./lib')
        return { on = { rang = function(event, ctx)
            local bell = ctx.get('one', 'bell')
            ctx.patch('one', 'bell', { rings = lib.twice(bell.rings) + event.payload.add })
            ctx.put('two', 'bell', { note = 'new' })
            ctx.put('aa.one', 'engine.position', { x = 1.5, layer = 1 })
            ctx.record('aa.rang', { at = ctx.tick, list = { 1, 2 } })
            ctx.remove('one')
            print('seen', #ctx.query('bell'), ctx.has('one', 'bell'))
        end } }";
    let payload = json!({ "add": 1 }).as_object().unwrap().clone();
    let event = Event {
        kind: "aa.rang",
        tick: 6,
        fact: 3,
        source: None,
        payload: &payload,
    };
    let out = run(script, Target::Event(&event));
    let commands = out.result.unwrap();
    assert_eq!(out.log, ["seen\t1\ttrue"]);
    let ops: Vec<String> = commands
        .iter()
        .map(|c| serde_json::to_string(c).unwrap())
        .collect();
    assert_eq!(
        ops,
        [
            r#"{"op":"patch","entity":"aa.one","component":"aa.bell","fields":{"rings":5}}"#,
            r#"{"op":"put","entity":"aa.two","component":"aa.bell","fields":{"note":"new","rings":0}}"#,
            r#"{"op":"put","entity":"aa.one","component":"engine.position","fields":{"height":0.0,"layer":1,"x":1.5,"y":0.0}}"#,
            r#"{"op":"record","kind":"aa.rang","payload":{"at":7,"list":[1,2]}}"#,
            r#"{"op":"remove","entity":"aa.one"}"#,
        ]
    );
}

#[test]
fn a_bad_write_aborts_the_call_at_its_line_even_if_caught() {
    for (script, code, needle) in [
        (
            "ctx.put('one', 'bell', { rings = 1.5 })",
            "bad_command",
            "expected int",
        ),
        (
            "ctx.put('one', 'bell', { nope = 1 })",
            "bad_command",
            "no field",
        ),
        (
            "ctx.put('engine.x', 'bell', {})",
            "bad_command",
            "own namespace",
        ),
        (
            "ctx.record('engine.start')",
            "bad_command",
            "own event kinds",
        ),
        (
            "ctx.put('one', 'engine.motion', {})",
            "bad_command",
            "writes",
        ),
        (
            "pcall(ctx.patch, 'one', 'bell', { rings = 'x' })",
            "bad_command",
            "expected int",
        ),
        ("while true do end", "over_budget", "budget"),
        (
            "local b = buffer.create(9000000)",
            "out_of_memory",
            "memory",
        ),
        ("error('boom')", "runtime_error", "boom"),
        (
            "for i = 1, 2000 do ctx.record('rang') end",
            "over_quota",
            "at most",
        ),
    ] {
        let out = cadence(&format!(
            "return {{ cadence = function(ctx)\n{script}\nend }}"
        ));
        let abort = out.result.unwrap_err();
        assert_eq!(abort.code, code, "{script}: {abort}");
        assert!(abort.message.contains(needle), "{script}: {abort}");
        if code != "over_budget" && code != "out_of_memory" {
            assert_eq!(
                abort.location.map(|l| (l.file, l.line)),
                Some(("main.luau".into(), Some(2))),
                "{script}"
            );
        }
    }
}

#[test]
fn no_state_survives_between_calls_and_randomness_is_seeded() {
    let m = module(
        "local seen = 0
        return { cadence = function(ctx)
            seen += 1
            ctx.record('rang', { seen = seen, r = ctx.random(), n = ctx.random_int(1, 6) })
        end }",
    );
    let registry = Registry::new(&[&m]).unwrap();
    let set = ScriptSet::load(&m, &registry).unwrap();
    let world = store();
    let calls = |seed: u64| {
        let mut rng = SimRng::for_key(seed, "t");
        (0..3)
            .map(|_| {
                set.call(0, Target::Cadence, &world, &registry, &mut rng)
                    .result
                    .unwrap()
            })
            .collect::<Vec<_>>()
    };
    let a = calls(1);
    assert_eq!(a, calls(1));
    assert_ne!(a, calls(2));
    for commands in &a {
        let Command::Record { payload, .. } = &commands[0] else {
            panic!()
        };
        assert_eq!(payload["seen"], 1);
    }
}

#[test]
fn loading_reports_every_problem() {
    let m = module(
        "return { on = { nope = function() end, rang = 3, ['engine.start'] = function() end },
                  cadence = true, extra = 1 }",
    );
    let registry = Registry::new(&[&m]).unwrap();
    let problems = ScriptSet::load(&m, &registry).err().unwrap();
    assert_eq!(problems.len(), 4, "{problems:?}");
    assert!(problems.iter().all(|p| p.code == "load_error"));

    let ok = module("return { on = { rang = function() end, ['engine.start'] = function() end } }");
    let set = ScriptSet::load(&ok, &Registry::new(&[&ok]).unwrap()).unwrap();
    let events: Vec<&str> = set.scripts[0].events.iter().map(String::as_str).collect();
    assert_eq!(events, ["aa.rang", "engine.start"]);
    assert!(!set.scripts[0].cadence);
}

#[test]
fn maths_is_portable() {
    let out = cadence(
        "return { cadence = function(ctx)
            ctx.record('rang', { s = math.sin(1), p = math.pow(2, 0.5), l = math.log(10) })
        end }",
    );
    let commands = out.result.unwrap();
    let Command::Record { payload, .. } = &commands[0] else {
        panic!()
    };
    assert_eq!(payload["s"], json!(libm::sin(1.0)));
    assert_eq!(payload["p"], json!(libm::pow(2.0, 0.5)));
    assert_eq!(payload["l"], json!(libm::log(10.0)));
}

#[test]
fn the_type_file_lists_every_member() {
    let types = crate::api::luau_types("9.9.9");
    for member in crate::api::CTX {
        assert!(types.contains(&format!("\t{}: {},", member.name, member.ty)));
    }
    assert!(types.contains("engine 9.9.9"));
}

#[test]
fn generalized_iteration_refuses_address_order_at_its_line() {
    // The reproduction from PR #56: a table keyed by tables, walked by `for ... in` alone.
    for key in ["{}", "function() end"] {
        let script = format!(
            "return {{ cadence = function(ctx)
                local objects = {{}}
                for i = 1, 30 do objects[{key}] = i end
                local order = {{}}
                for object, value in objects do table.insert(order, value) end
                ctx.record('rang', {{ order = table.concat(order, ',') }})
            end }}"
        );
        let first = cadence(&script).result.unwrap_err();
        assert_eq!(first.code, "runtime_error", "{first}");
        assert!(first.message.contains("as a key"), "{first}");
        assert_eq!(
            first.location.as_ref().map(|l| (l.file.as_str(), l.line)),
            Some(("main.luau", Some(5)))
        );
        assert_eq!(cadence(&script).result.unwrap_err(), first);
    }
}

#[test]
fn generalized_iteration_over_stable_keys_still_works() {
    let out = cadence(
        "return { cadence = function(ctx)
            local t = { 'a', 'b', 'c', x = 1, [true] = 2, [2.5] = 3 }
            local seen, via_pairs = {}, {}
            for k, v in t do table.insert(seen, tostring(k) .. '=' .. tostring(v)) end
            for k, v in pairs(t) do table.insert(via_pairs, tostring(k) .. '=' .. tostring(v)) end
            local words = {}
            for w in string.gmatch('one two', '%a+') do table.insert(words, w) end
            local custom = setmetatable({}, { __iter = function() return next, { 5, 6 } end })
            local sum = 0
            for _, v in custom do sum += v end
            for i, v in ipairs({ 1, 2 }) do sum += v end
            ctx.record('rang', {
                same = table.concat(seen, ',') == table.concat(via_pairs, ','),
                n = #seen, words = table.concat(words, ','), sum = sum,
            })
        end }",
    );
    let commands = out.result.unwrap();
    let Command::Record { payload, .. } = &commands[0] else {
        panic!("{commands:?}")
    };
    assert_eq!(payload["same"], true);
    assert_eq!(payload["n"], 6);
    assert_eq!(payload["words"], "one,two");
    assert_eq!(payload["sum"], 14);
}

#[test]
fn a_loop_that_cant_be_wrapped_fails_to_load_at_its_line() {
    for (script, line) in [
        // An if-expression in a function literal hides where the loop's iterator ends.
        (
            "local function f()\nfor k in function() return if k then nil else nil end do end\nend\ndo end\nreturn {}",
            2,
        ),
        ("local x = 1\nlocal __galaxy_iterate = 2\nreturn {}", 2),
    ] {
        let m = module(script);
        let problems = ScriptSet::load(&m, &Registry::new(&[&m]).unwrap())
            .err()
            .unwrap();
        assert_eq!(problems[0].code, "load_error", "{problems:?}");
        assert_eq!(
            problems[0].location.as_ref().map(|l| l.line),
            Some(Some(line)),
            "{problems:?}"
        );
    }
}

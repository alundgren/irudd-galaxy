//! The `ctx` API scripts call, declared once. The host binds exactly these names
//! ([`crate::host`], checked by a test) and [`luau_types`] writes the Luau type file that
//! installs ship as `galaxy.d.luau`, so the types can't drift from what the host offers.

/// One member of `ctx`: its name, Luau type and what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub name: &'static str,
    pub ty: &'static str,
    pub doc: &'static str,
}

/// Everything `ctx` holds, in the order the type file lists it.
pub const CTX: &[Member] = &[
    Member {
        name: "tick",
        ty: "number",
        doc: "The tick being run.",
    },
    Member {
        name: "module",
        ty: "string",
        doc: "This module's ID.",
    },
    Member {
        name: "get",
        ty: "(entity: Id, component: Id) -> Fields?",
        doc: "A copy of a component's fields, or nil if the entity doesn't have it. Reads see \
              the world as it was when this call started.",
    },
    Member {
        name: "has",
        ty: "(entity: Id, component: Id) -> boolean",
        doc: "Whether the entity has the component.",
    },
    Member {
        name: "query",
        ty: "(component: Id) -> { Id }",
        doc: "The entities that have the component, in ID order.",
    },
    Member {
        name: "put",
        ty: "(entity: Id, component: Id, fields: Fields?) -> ()",
        doc: "Sets a whole component; fields left out get their defaults. Creates the entity \
              if it is new (only in this module's namespace).",
    },
    Member {
        name: "patch",
        ty: "(entity: Id, component: Id, fields: Fields) -> ()",
        doc: "Changes the given fields of a component the entity has.",
    },
    Member {
        name: "remove",
        ty: "(entity: Id, component: Id?) -> ()",
        doc: "Removes a component, or the whole entity when no component is given.",
    },
    Member {
        name: "record",
        ty: "(kind: Id, payload: Payload?) -> ()",
        doc: "Records a fact of one of this module's event kinds. It goes into the world's \
              history, and handlers for its kind run on the next tick.",
    },
    Member {
        name: "random",
        ty: "() -> number",
        doc: "A seeded random number in [0, 1), from this module's own stream.",
    },
    Member {
        name: "random_int",
        ty: "(lo: number, hi: number) -> number",
        doc: "A seeded random whole number from lo to hi, both included.",
    },
];

/// The Luau type file for module scripts.
pub fn luau_types(engine: &str) -> String {
    let mut out = format!(
        "--!strict\n\
         -- Luau types for module scripts, from engine {engine}.\n\
         -- Generated from the engine's ctx declaration; do not edit.\n\
         \n\
         --- A component type, event kind or entity: `module.name`, or a plain `name` for\n\
         --- this module's own.\n\
         export type Id = string\n\
         \n\
         --- A component's fields. An `id` field holds an entity ID, or \"\" for none.\n\
         export type Fields = {{ [string]: number | boolean | string }}\n\
         \n\
         --- A fact's payload: numbers, booleans, text, lists and tables.\n\
         export type Payload = {{ [string]: any }}\n\
         \n\
         --- An event: a fact recorded on an earlier tick, or an input on this one.\n\
         export type Event = {{\n\
         \tkind: Id,\n\
         \ttick: number,\n\
         \tfact: number,\n\
         \tsource: string?,\n\
         \tpayload: Payload,\n\
         }}\n\
         \n\
         --- What a handler reads and writes through. Writes apply together when the\n\
         --- handler returns, or not at all if it fails.\n\
         export type Ctx = {{\n"
    );
    for member in CTX {
        out.push_str(&format!(
            "\t--- {}\n\t{}: {},\n",
            member.doc, member.name, member.ty
        ));
    }
    out.push_str(
        "}\n\
         \n\
         --- What a script file returns. `on` handlers run when an event of their kind\n\
         --- happens; `cadence` runs on the slow cadence (once per second of game time).\n\
         --- Nothing runs every frame.\n\
         export type Script = {\n\
         \ton: { [Id]: (event: Event, ctx: Ctx) -> () }?,\n\
         \tcadence: ((ctx: Ctx) -> ())?,\n\
         }\n\
         \n\
         return {}\n",
    );
    out
}

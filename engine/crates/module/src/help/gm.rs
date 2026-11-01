//! galaxy-gm's commands, and the guide's topics.

use super::{Command, Topic};
use crate::discover::Tool;

pub static COMMANDS: &[Command] = &[
    Command {
        tool: Tool::Gm,
        path: "guide",
        synopsis: "[topic]",
        answers: "What do you want to do, and which command does it? For both galaxy and \
                  galaxy-gm, with an example each.",
        about: "",
        args: &[(
            "[topic]",
            "play, notes, replay, motion, modules, style or install; without one, every task \
             in a line each",
        )],
        output: &[],
        writes: "",
        example: "galaxy-gm guide notes",
        next: "Run the command it names; `<command> --help` for its options.",
        topic: "install",
    },
    Command {
        tool: Tool::Gm,
        path: "check",
        synopsis: "<module>",
        answers: "Does this module load? Its manifest, start state, scripts, models and \
                  style, and whether what its start state wears exists.",
        about: "The modules beside it are loaded too, since a module may use another's types \
                and events. Findings exit 5 with kind findings.",
        args: &[("<module>", "a module folder (with a manifest.toml)")],
        output: &[
            ("ok", "no findings"),
            ("module, version", "the module checked"),
            (
                "context_modules",
                "the modules beside it that were loaded with it",
            ),
            (
                "findings",
                "each problem: kind (schema, script, start or look) and message",
            ),
            ("notes", "modules beside it that could not be read, skipped"),
            (
                "worn.looks, worn.models",
                "each look and model the module has, and the things (from every \
              module's start state) that wear it: restyling one look changes all of them",
            ),
        ],
        writes: "",
        example: "galaxy-gm check game/modules/core",
        next: "Fix each finding; galaxy-gm fields lists the keys style.toml, manifest.toml and \
               the engine's components take, and galaxy-gm types the script API.",
        topic: "modules",
    },
    Command {
        tool: Tool::Gm,
        path: "types",
        synopsis: "",
        answers: "What can a module's Luau scripts call? Prints the type file (plain text) \
                  for luau-analyze and for reading.",
        about: "",
        args: &[],
        output: &[],
        writes: "",
        example: "galaxy-gm types",
        next: "galaxy-gm check <module> runs the scripts' type check.",
        topic: "modules",
    },
    Command {
        tool: Tool::Gm,
        path: "fields",
        synopsis: "[style | components | manifest | inputs | spec] [--component <name>] \
                   [--modules <dir>]",
        answers: "Which keys and fields exist, with their types, defaults and ranges? For \
                  style.toml, the engine's components (and a module's own), manifest.toml, \
                  inputs files and motion check specs.",
        about: "Generated from the engine's own definitions, so it is what this build \
                accepts. Without a topic, a one-line summary of each.",
        args: &[
            ("[topic]", "style, components, manifest, inputs or spec"),
            (
                "--component <name>",
                "components only: just this one, by full name or bare (flight for \
              engine.flight)",
            ),
            (
                "--modules <dir>",
                "components or style only. With components: also the components these \
              modules declare. With style: also their looks and models, and which things \
              wear each (`worn`)",
            ),
        ],
        output: &[
            ("topic", "which file or thing the fields belong to"),
            (
                "tables or components",
                "each table (or component) and its fields: name, type, \
              default, range or values, and what it does",
            ),
            (
                "worn.looks, worn.models",
                "style with --modules: each look and model, and the things whose start \
              state wears it",
            ),
        ],
        writes: "",
        example: "galaxy-gm fields style",
        next: "Edit the file, then galaxy-gm check <module>.",
        topic: "style",
    },
    Command {
        tool: Tool::Gm,
        path: "model",
        synopsis: "<module> [<model>]",
        answers: "What is a model made of? Its parts (glTF nodes) with sizes and places, \
                  its outline as collision sees it, its morph target, and which parts glow or \
                  carry a bite, without reading the glTF file.",
        about: "Sizes are world units at scale 1, in the model's own frame: x its front (where \
                its thing heads), y its left, z up; a thing's engine.model scale multiplies \
                them. The outline is the model's convex footprint on the plane, which \
                collision uses unless the thing's engine.body has a radius. Without a model, \
                every model the module has, a line each, with the things that wear it.",
        args: &[
            ("<module>", "a module folder (with a manifest.toml)"),
            (
                "[model]",
                "a model, by name (hub for models/hub.gltf) or ID (drift.hub)",
            ),
        ],
        output: &[
            (
                "models",
                "without a model: each one's file, parts, triangles, size and who wears it",
            ),
            (
                "size, triangles, radius",
                "the box round the model (size at its centre), its triangles, and how far \
              it reaches from its origin in 3D",
            ),
            (
                "parts",
                "a line a part (glTF node): name, triangles, size at its centre, and its \
              glow, bite and morph when it has them; thin parts are marked",
            ),
            (
                "outline.reach",
                "collision's footprint: how far it reaches from its centre front, left, \
              back and right, the most and the least",
            ),
            (
                "morph",
                "the parts the morph target moves and how far, and the outline at morph 1; \
              or none",
            ),
            (
                "glow, bite",
                "the parts that glow or carry a bite, and how much",
            ),
            (
                "worn_by",
                "the things whose start state (in this module and those beside it) wears \
              it, with their scale",
            ),
        ],
        writes: "",
        example: "galaxy-gm model game/modules/core hub",
        next: "galaxy-gm motion flicker names these parts under changed pixels; galaxy shot \
               --crop <thing> draws one thing close up.",
        topic: "modules",
    },
    Command {
        tool: Tool::Gm,
        path: "notes",
        synopsis: "[<recording | session folder | sessions folder>] [--all]",
        answers: "What did the player note with F8? Each note's number, tick, time and text, \
                  with the frame they saw, newest sessions first.",
        about: "Without a path it reads the sessions folder every `galaxy play` records into \
                (`galaxy doctor` names it; GALAXY_SESSIONS moves it). Sessions without notes \
                are counted, not listed.",
        args: &[
            (
                "[path]",
                "a recording, a session folder, or a folder of sessions; the sessions \
              folder by default",
            ),
            ("--all", "every note; 20 at most by default"),
        ],
        output: &[
            (
                "sessions",
                "each session with notes, newest first: session (folder), \
              recording, build, modules, ticks and seconds played, notes",
            ),
            (
                "sessions.notes",
                "note (its number, for audit and film --note), tick, t \
              (seconds), text, seen (the frame shown), drawn_again (audit's picture, once \
              audited)",
            ),
            ("more", "how many notes were left out, when capped"),
            ("sessions_without_notes", "how many sessions have none"),
        ],
        writes: "",
        example: "galaxy-gm notes",
        next: "galaxy-gm audit <session> checks each note's world and draws it again; \
               galaxy-gm film <session> --note <n> films the seconds round it.",
        topic: "notes",
    },
    Command {
        tool: Tool::Gm,
        path: "recording",
        synopsis: "<recording> [summary | timeline | inputs | orders | zooms] \
                   [--from <tick>] [--to <tick>] [--name <text>] [--all]",
        answers: "What is in a recording, and what did the player do? Its summary, one \
                  timeline of the session, or every input, order and arrival, or zoom, a line \
                  each.",
        about: "timeline answers \"what did the player do?\" in one list, times as m:ss.s with \
                the tick: the orders the player gave (an order changed on a tick an input \
                came, with that input), each marked superseded or arrived; orders that changed \
                with no input (waypoint steps, scripts), a run of them a line; arrivals; \
                notes; zoom bursts, a line each; and other keys, a run of them a line.\n\
                timeline and orders replay the recording on this build to find each change of \
                an entity's engine.steer (an order given, changed or dropped) and each \
                arrival; `replay_identical` says whether the replay ended in the recorded \
                world (if not, run it on the recording's build: galaxy-gm --build <name> ...).",
        args: &[
            ("<recording>", "a recording, or a session folder"),
            (
                "[view]",
                "summary (the default), timeline, inputs, orders or zooms",
            ),
            (
                "--from <tick>, --to <tick>",
                "only rows between these ticks (30 a second)",
            ),
            (
                "--name <text>",
                "inputs only: just the inputs whose name contains this (order for \
              drift.order_dock); several, comma-separated",
            ),
            ("--all", "every row; 40 at most by default"),
        ],
        output: &[
            (
                "summary",
                "build, seed, modules and versions, ticks (start, end), seconds, \
              counts of inputs, notes, zooms and hot-loads, inputs_by_name, end_hash",
            ),
            (
                "rows",
                "timeline: `0:01.0 (tick 31) order drift.ship: dock drift.berth (from \
              drift.order_dock down); arrived at 0:04.3`; inputs: \
              `tick 31 t 1.03 keyboard drift.order_dock down`; orders: \
              `tick 31 t 1.03 order drift.ship: dock drift.berth` or `arrived ...`; zooms: \
              the view height the wheel set",
            ),
            (
                "count, more",
                "how many rows there are, and how many were left out",
            ),
            (
                "replay_identical",
                "timeline and orders: the replay ended in the recorded world",
            ),
            ("legend", "timeline: what each kind of line means"),
        ],
        writes: "",
        example: "galaxy-gm recording sessions/2026-11-01-142233-drift timeline --from 300",
        next: "galaxy-gm film <recording> --from <tick> --to <tick> films a moment; \
               galaxy-gm notes <recording> lists its notes.",
        topic: "replay",
    },
    Command {
        tool: Tool::Gm,
        path: "film show",
        synopsis: "<film> [--thing <x,y>] [--from <t>] [--to <t>] [--every <n>] \
                   [--fields <list>] [--gap <thing>] [--lines] [--all]",
        answers: "Where were things, frame by frame, in a film? Its probes as a table, a \
                  frame a line, without reading film.json, frames.jsonl or lines.jsonl.",
        about: "Also prints the film's summary: fps, size, things and its changes (inputs, \
                orders, arrivals).",
        args: &[
            ("<film>", "a film folder"),
            (
                "--thing <x,y>",
                "things to show, by ID or bare name; the followed one by \
              default",
            ),
            (
                "--from <t>, --to <t>",
                "only frames between these times, seconds; a time between two \
              frames (or the same time twice) shows the nearest frame, and `snapped` says so",
            ),
            ("--every <n>", "every nth frame"),
            (
                "--fields <list>",
                "of x, y, angle, height, screen, on_screen; x,y,angle,screen \
              by default",
            ),
            (
                "--gap <thing>",
                "add each shown thing's gap to this thing's surface, outline to outline \
              as collision measures it (`<x>.gap_to_<thing>`, world units: below 0 \
              overlaps), to look at the frames round an overlap `track --to` found",
            ),
            (
                "--lines",
                "the lines each frame drew instead: kind, points, dots, alpha, width",
            ),
            ("--all", "every row; 30 at most by default"),
        ],
        output: &[
            ("columns", "what each row holds"),
            (
                "rows",
                "a frame a line: frame, t (seconds), tick, alpha (how far between the \
              tick before and this one), then each thing's fields",
            ),
            (
                "units",
                "x, y, height in world units, angle in radians counter-clockwise from \
              +x, screen in pixels from the top left",
            ),
            (
                "things, changes",
                "every thing in the film; inputs, orders and arrivals",
            ),
            ("more", "rows left out"),
            (
                "snapped",
                "the time asked for had no frame: which frame is shown instead",
            ),
        ],
        writes: "",
        example: "galaxy-gm film show films/ram --thing ship --gap hub --from 2 --to 2.5",
        next: "galaxy-gm motion track <film> --thing <x> measures how it moves.",
        topic: "motion",
    },
    Command {
        tool: Tool::Gm,
        path: "audit",
        synopsis: "<recording>",
        answers: "Replay a played session to each of its notes: is the world the one the \
                  player had, and does the moment draw the same again?",
        about: "Which build draws: without --build, the launcher runs the build that \
                made the recording (when it is installed), so the picture is what the player \
                saw. `galaxy-gm --build <name> audit ...` draws with that build instead: a \
                drawing defect fixed or added since then shows differently. `builds` always \
                says which build recorded the session and which drew this, and its note says \
                plainly when they differ and how to see what the player saw. At each note it compares the world's hash with the note's and draws the moment again, between the same \
                two ticks with the same camera, beside the frame that was seen. Pictures are \
                identical on the machine that played and should visibly match on another GPU. \
                A world that differs exits 5 with kind replay_diverged.",
        args: &[(
            "<recording>",
            "a recording, or a session folder (its recording.json is used)",
        )],
        output: &[
            (
                "notes",
                "one entry a note: note (its number), tick, text, identical (the world \
              matches the note's), seen (the frame the player saw, notes/0001.png), drawn \
              (the moment drawn again, audit/0001.png), camera_matches, mean_difference (per \
              channel, 0 to 255) and pixels_differing (the share of pixels, 0 to 1)",
            ),
            (
                "builds",
                "recorded (the build that made the session), drawn (this build), same \
              (true, false, or null when it can't be told), note (what that means; when they \
              differ, how to run the recording's build)",
            ),
        ],
        writes: "<session>/audit/0001.png, one a note",
        example: "galaxy-gm audit sessions/2026-11-01-142233-drift/recording.json",
        next: "Open notes/0001.png and audit/0001.png; galaxy-gm film <recording> --note 1 \
               films the seconds round it.",
        topic: "notes",
    },
    Command {
        tool: Tool::Gm,
        path: "film",
        synopsis: "<recording> (--note <n> | --from <tick> --to <tick>) [options]",
        answers: "Film a played session again round one of its notes (or between two ticks), \
                  frame by frame as the window showed it; by this build, the recording's or two.",
        about: "Which build draws: this one, unless told. --as-recorded draws with the build \
                that recorded the session (installed builds sit side by side; galaxy-gm doctor \
                lists them): this build runs that one's galaxy-gm to draw, then stamps the \
                film with the build that drew it and adds what an older build doesn't record \
                (the film's changes, things' reach, outlines for surface gaps) from its own \
                replay of the same span, so every tool reads it. --builds a,b films the same \
                span with two builds into <out>/<build> each and compares them (FLIP). \
                Older builds will always lack newer commands and options: they are asked only \
                for what every build since films began takes, and a build that predates films \
                says so. `builds` says which build recorded the session and which drew this, \
                and its note says plainly when they differ. Every note inside the film is \
                checked against the world's hash it kept, so a film of a different world exits \
                5 (replay_diverged) rather than pass for what the player saw. By default the \
                film is at the note's own frame size (window scale included), which is what \
                catches thin lines breaking and one-frame flicker.",
        args: &[
            ("<recording>", "a recording, or a session folder"),
            ("--note <n>", "the note to film round, counting from 1"),
            ("--before <s>", "seconds before the note; 3 by default"),
            ("--after <s>", "seconds after the note; 1 by default"),
            (
                "--from <tick>, --to <tick>",
                "ticks to film instead of a note (30 a second)",
            ),
            ("--fps <n>", "frames a second, 1 to 240; 60 by default"),
            (
                "--size <w>x<h>",
                "pixels; the note's frame size by default, else 960x540",
            ),
            (
                "--out <folder>",
                "where the film goes; films/note-0001 (or \
              films/ticks-<from>-<to>) beside the recording by default",
            ),
            (
                "--probes-only",
                "no pictures and no GPU: positions on screen only",
            ),
            (
                "--as-recorded",
                "draw with the build that recorded the session, as the player saw it",
            ),
            (
                "--builds <a,b>",
                "film with two builds and compare: each `recorded`, `this`, an installed \
              build's name, or a part only one has (its commit); recorded,this shows what a \
              fix changed",
            ),
        ],
        output: &[
            ("out", "the film folder"),
            ("frames, fps, size, drawn", "how it was filmed"),
            (
                "from, to",
                "the first and last frame: t (seconds since the session started) \
              and tick",
            ),
            (
                "notes_checked",
                "each note inside the film: tick, identical (the replayed \
              world is the player's). The world can be identical on another build while \
              the picture differs: read builds too",
            ),
            (
                "builds",
                "recorded, drawn, same, note: as for audit; every motion tool's report \
              repeats it",
            ),
            (
                "drawn_by, added",
                "with another build: which drew it, and what this build added",
            ),
            (
                "films, compare",
                "with --builds: each film (build, out) and where they differ: \
              reading, visibly_different_frames, image",
            ),
            (
                "behaviour",
                "with --builds: how the followed thing moved on each build, side by \
              side (path, top speed, arrival, end) and whether it is the same",
            ),
        ],
        writes: "the film folder (--out): film.json (how it was filmed, small), frames.jsonl \
                 and lines.jsonl (probes, a frame a line; galaxy-gm film show reads them), \
                 frames/00000.png, and motion/ for the tools' pictures",
        example: "galaxy-gm film sessions/2026-11-01-142233-drift/recording.json --note 1",
        next: "galaxy-gm motion flicker <folder> for pops and flicker, track for how a thing \
               moves, sheet to see frames; galaxy-gm motion --help lists all.",
        topic: "notes",
    },
    Command {
        tool: Tool::Gm,
        path: "motion check",
        synopsis: "<spec> [--out <folder>]",
        answers: "Do the motion expectations in this spec still hold? Films what the spec says \
                  and runs each tool against its bounds.",
        about: "A spec is a TOML file: what to film (modules, inputs, ticks, size) and \
                [[expect]] entries, each a tool, a value from its report (dotted, such as \
                gap.overshoot) and bounds (at_least, at_most or equals); any other key is an \
                option for the tool. `galaxy-gm fields spec` lists the keys. Expectations that \
                don't hold exit 5 with kind motion_expectations.",
        args: &[
            (
                "<spec>",
                "a spec file; its modules and inputs are relative to it",
            ),
            (
                "--out <folder>",
                "where the film goes; by default galaxy-motion-check/<spec \
              name> in the OS temp folder (the report's `film` says where)",
            ),
        ],
        output: &[
            ("ok", "every expectation held"),
            ("film", "the film folder it made"),
            (
                "expectations",
                "one line each: tool, value, the bounds, got (the value \
              measured), pass, the tool's reading and picture",
            ),
        ],
        writes: "the film (--out) and each tool's picture in <film>/motion/",
        example: "galaxy-gm motion check course.toml --out films/course",
        next: "When one fails, run that tool on the film for the whole report and picture.",
        topic: "motion",
    },
    Command {
        tool: Tool::Gm,
        path: "doctor",
        synopsis: "",
        answers: "Which engine is this, which builds and launchers are installed, and which \
                  modules are near the working folder?",
        about: "",
        args: &[],
        output: &[
            ("engine, build", "this engine's version and installed build"),
            (
                "installed_builds",
                "each installed build: name, version, dev",
            ),
            (
                "launchers",
                "the launcher folder, which build made it, whether it is on PATH",
            ),
            (
                "modules",
                "modules near the working folder, with their minimum engine",
            ),
        ],
        writes: "",
        example: "galaxy-gm doctor",
        next: "galaxy-gm --build <name> <command> runs a named build, such as a dev build.",
        topic: "install",
    },
];

/// The guide. Tasks name commands as `gm <path>` or `game <path>`.
pub static TOPICS: &[Topic] = &[
    Topic {
        name: "play",
        title: "Playing, and taking notes",
        about: "`galaxy play` runs modules in a window. Every session records itself into a \
                folder of its own under the sessions folder: recording.json (everything \
                needed to run it again), save.db (the world at the end) and notes/0001.png. \
                F8 pauses and opens a note box over the frame on screen; Enter keeps the note \
                with its tick, the world's hash and the camera. A bot plays an inputs file \
                through the same window loop (--inputs), and its [[note]] entries are taken \
                like F8 notes.",
        tasks: &[
            ("Play the game", "game play"),
            (
                "Draw one frame headless, or one thing (--crop)",
                "game shot",
            ),
        ],
    },
    Topic {
        name: "notes",
        title: "The player's F8 notes",
        about: "A note is text the player typed, the tick it was on (30 ticks a second), the \
                world's hash then, and the frame that was on screen (notes/0001.png in the \
                session folder). To act on one: read it, see the frame, check the replay \
                reaches the same world (audit draws the moment again into audit/0001.png), \
                then film the seconds round it and measure what moved.",
        tasks: &[
            ("List the player's notes", "gm notes"),
            ("Replay to each note and draw it again", "gm audit"),
            ("Film the seconds round a note", "gm film"),
        ],
    },
    Topic {
        name: "replay",
        title: "Recordings, replays and the world's state",
        about: "A recording holds the build that made it, the exact module files, the world it \
                started from, every input, hot-load, note and zoom by tick, and the hash it \
                ended with. Replaying it on the same build gives the same world bit for bit; \
                the launcher picks that build for replay and audit. Ticks are 30 a second; the world's hash says \
                whether two worlds are the same. Inputs files script a run: [[input]] (tick, \
                name, source, payload), [[note]], [[hot_load]] and [[zoom]].",
        tasks: &[
            ("Run a world headless and read its state", "game sim run"),
            ("Carry on from a save", "game sim load"),
            (
                "What the player did, in order: the timeline",
                "gm recording",
            ),
            (
                "Check a recording replays to the same world",
                "game sim replay",
            ),
        ],
    },
    Topic {
        name: "motion",
        title: "How motion looks",
        about: "A screenshot can't show how something moves. Film a short span frame by frame \
                as the window would draw it (60 fps, at the player's size: thin lines \
                breaking and one-frame pops are missed at 30 fps or small sizes), then ask one \
                question per tool. Each answers with a `reading` sentence, a few numbers, the \
                times (t, in seconds; tick) of suspicious moments and one picture. Probes-only \
                films need no GPU and serve track and dots. When a defect is fixed, keep it \
                fixed with a motion check spec.",
        tasks: &[
            ("Film a scripted run", "game film"),
            ("Film a played session round a note", "gm film"),
            (
                "Film it as the player saw it (the recording's build), or on two builds \
                 and compare: film --as-recorded, film --builds recorded,this",
                "gm film",
            ),
            ("Where things are, frame by frame", "gm film show"),
            (
                "How far and how fast did a thing move, and when? Film with --probes-only \
                 (no GPU), then track --thing <x>: displacement, path, top speed with times",
                "gm motion track",
            ),
            (
                "Does a thing follow smoothly, overshoot, settle?",
                "gm motion track",
            ),
            ("Do dotted lines stay put? (no GPU)", "gm motion dots"),
            ("Does a pattern crawl along a path?", "gm motion strip"),
            (
                "See frames side by side, or an onion skin",
                "gm motion sheet",
            ),
            ("Does anything pop, blip or flicker?", "gm motion flicker"),
            ("Does a turning thing's shading flip?", "gm motion tone"),
            ("Is something moving that has no name?", "gm motion flow"),
            (
                "What changed against an approved film?",
                "gm motion compare",
            ),
            ("Keep a motion expectation as a test", "gm motion check"),
        ],
    },
    Topic {
        name: "modules",
        title: "Modules: checking them and what scripts can use",
        about: "A module is a folder with manifest.toml (id, versions, scripts, the events and \
                components it declares, what it writes, keys), start.toml (its entities), \
                scripts/*.luau, models/*.gltf and style.toml. IDs are `<module>.<name>`. \
                Scripts run on events and once a second, never per frame; the engine's \
                services fly, move and collide things.",
        tasks: &[
            ("Check a module loads", "gm check"),
            ("What scripts can call", "gm types"),
            (
                "What a model is made of: parts, outline, morph, glow",
                "gm model",
            ),
        ],
    },
    Topic {
        name: "style",
        title: "How the world is drawn, and the keys files take",
        about: "style.toml sets the scene-wide ink, light, sky and course (from one module \
                only) and named looks a model can wear (`look = \"<module>.<name>\"`). Every \
                key is optional. Check a change with galaxy shot (a still) or a film and a \
                motion tool (for anything that turns or moves).",
        tasks: &[(
            "Which keys and fields exist; which things wear a look",
            "gm fields",
        )],
    },
    Topic {
        name: "install",
        title: "Builds and this install",
        about: "Builds install side by side. The launchers (galaxy, galaxy-gm) pick the newest \
                build the modules allow, or the build a recording names for replay and audit; \
                `--build <name>` first runs a named one, such as a dev build. film draws with \
                another installed build itself (--as-recorded, --builds), so older builds \
                need no newer options. A module's manifest names the oldest engine it runs on.",
        tasks: &[
            ("Which builds are installed", "gm doctor"),
            ("Which engine galaxy runs", "game doctor"),
            ("The task list, or one topic", "gm guide"),
        ],
    },
];

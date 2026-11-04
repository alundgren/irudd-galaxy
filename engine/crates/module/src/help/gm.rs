//! galaxy-gm's commands, and the guide's topics.

use super::{Command, Recipe, Topic};
use crate::discover::Tool;

pub static COMMANDS: &[Command] = &[
    Command {
        tool: Tool::Gm,
        path: "audio",
        synopsis: "<command>",
        answers: "Inspect, process, loop, mix, judge or check audio.",
        about: "Offline authoring tools; one JSON object. Default output summarizes windows and repeated findings; --detail shows every window. Numeric tools are local. Only judge makes paid OpenRouter calls. Undefined measurements are null with reasons.",
        args: &[],
        output: &[(
            "reading, findings, detail",
            "interpretation, timestamped findings with actions, and method",
        )],
        writes: "",
        example: "galaxy-gm audio inspect clip.wav",
        next: "galaxy-gm guide audio walks through the workflow.",
        topic: "audio",
    },
    Command {
        tool: Tool::Gm,
        path: "audio inspect",
        synopsis: "<clip|module> [--loop | --sound <name>] [--target-lufs <n>] [--reference <clip>] [--detail]",
        answers: "Measure loudness, spectrum, defects and declared loops.",
        about: "WAV, Ogg Vorbis or MP3; mono/stereo, 8..192 kHz. Source LUFS differs from playback LUFS. Spectral flags need a reference and listening. File seams require --loop; module --sound reads actual loop settings and renders the seam as played. DC: dc_mean/removed_dc are whole-clip signed means; dc_max is the largest absolute per-second mean, all in linear full-scale units.",
        args: &[
            ("--loop", "declare a file to be a hard-wrapped loop"),
            (
                "--sound <name>",
                "inspect local sound in module folder; playback_seam uses its crossfade",
            ),
            ("--detail", "include all windows and individual findings"),
            (
                "--target-lufs <n>",
                "optional source target, -70..0 LUFS; flag above it",
            ),
            (
                "--reference <clip>",
                "compare gain-invariant spectral fractions and report loudness difference",
            ),
        ],
        output: &[
            (
                "loudness",
                "integrated, momentary, short-term LUFS, true peak, RMS, crest and range",
            ),
            (
                "spectrum, seam, playback_seam, stereo, defects",
                "summary, energy fractions low_mid 200-500 Hz, presence 2-5 kHz, air above 8 kHz; strongest 12 peaks with level_dbfs/prominence_db (--detail for all), loop boundaries",
            ),
            (
                "findings",
                "each has kind, start_s/end_s, numeric value and suggested_action",
            ),
        ],
        writes: "",
        example: "galaxy-gm audio inspect clip.wav --target-lufs -23",
        next: "For harshness locate peaks, band energy and attacks before choosing EQ; use mix for playback levels.",
        topic: "audio",
    },
    Command {
        tool: Tool::Gm,
        path: "audio process",
        synopsis: "<clip> --out <new.wav> [options]",
        answers: "Repair a clip into a new file and measure before and after.",
        about: "Order: trim, silence trim, isolated click interpolation, DC mean removal, EQ, fades, gain. Gentle EQ gains are -6..6 dB; notch cuts only. Gain respects the peak ceiling; no hidden limiter. Existing files are refused.",
        args: &[
            ("--detail", "include all windows and individual findings"),
            (
                "--out <new.wav>",
                "required new float WAV; never overwrites",
            ),
            (
                "--target-lufs <n>",
                "source target -70..0; requires 400 ms above gate; conflicts with gain-db",
            ),
            ("--peak <dBTP>", "true-peak ceiling -30..0, default -1"),
            ("--gain-db <n>", "gain -60..30 dB, constrained by peak"),
            ("--trim <start,end>", "keep this range in seconds"),
            ("--trim-silence", "trim below -60 dBFS with a 10 ms margin"),
            ("--fade-in <seconds>", "linear fade at start"),
            (
                "--fade-out <seconds>",
                "linear fade at end; use loop for repeating beds",
            ),
            (
                "--remove-dc",
                "subtract whole-clip signed channel means, linear full-scale units",
            ),
            ("--declick", "interpolate isolated impulses"),
            ("--high-shelf <Hz,dB>", "gentle shelf above frequency"),
            ("--low-shelf <Hz,dB>", "gentle shelf below frequency"),
            ("--notch <Hz,dB,Q>", "narrow peaking cut; Q 0.3..20"),
        ],
        output: &[
            (
                "before, after",
                "source summaries, after decoding the new file; --detail includes windows",
            ),
            (
                "edits, gain",
                "applied operations, requested/applied gain, peak_limited flag",
            ),
        ],
        writes: "<new.wav> only",
        example: "galaxy-gm audio process clip.wav --out soft.wav --target-lufs -23",
        next: "Listen and inspect the output; loop it separately if needed.",
        topic: "audio",
    },
    Command {
        tool: Tool::Gm,
        path: "audio loop",
        synopsis: "<clip> --out <new.wav> [--blend <s>] [--search <s>] [--curve <name>]",
        answers: "Find compatible endpoints and blend a tail into its head.",
        about: "Waveform correlation and level matching, then sample/slope refinement; no beat or harmony guarantee. match_score is 1-correlation + absolute level difference/20 dB: 0 best, lower better, unbounded above; not a probability. Shortens and rotates the clip. Re-decodes and checks three repetitions. Set loop_crossfade=0 in audio.toml for this prepared loop.",
        args: &[
            ("--detail", "include all windows and individual findings"),
            ("--out <new.wav>", "required new float WAV"),
            (
                "--blend <seconds>",
                "overlap 0.005..10, default 1; clip must be at least four overlaps long",
            ),
            (
                "--search <seconds>",
                "endpoint search 0..10, default 1; zero retains endpoints",
            ),
            (
                "--curve <name>",
                "linear (default) for correlated material or equal-power",
            ),
            ("--target-lufs <n>", "optional source target -70..0"),
            ("--peak <dBTP>", "peak ceiling -30..0, default -1"),
        ],
        output: &[
            (
                "before, after, edits, gain",
                "measurements, chosen endpoints and applied gain",
            ),
            (
                "loop_validation",
                "chronological boundaries, wraps and repeated-clip click timestamps",
            ),
        ],
        writes: "<new.wav> only",
        example: "galaxy-gm audio loop soft.wav --out bed.wav --blend 2 --search 1",
        next: "Audition at least three repetitions; numeric continuity cannot prove musical continuity.",
        topic: "audio",
    },
    Command {
        tool: Tool::Gm,
        path: "audio mix",
        synopsis: "<scenario.toml> [--out <new.wav>] [--stems <new-folder>]",
        answers: "Measure source loudness and each bus in the real mix.",
        about: "Scenario TOML: modules path, ticks (1..108000), optional seed=1 and inputs path, relative to scenario. Event inputs only. Uses the same Scene and Mixer as play, including voice limits, fades and master gain.",
        args: &[
            ("--detail", "include all windows and individual findings"),
            (
                "--out <new.wav>",
                "optional stereo float WAV of final playback; refuses existing file",
            ),
            (
                "--stems <new-folder>",
                "optional sound/music/ambience float WAVs before shared clamp; new directory",
            ),
        ],
        output: &[
            (
                "sources, playback, buses",
                "source levels, actual clamped mix and simultaneous post-gain bus levels",
            ),
            (
                "pre_clamp, windows",
                "pre-clamp peak and levels; summary; --detail has 1 s windows, rms_dbfs keyed by sound/music/ambience",
            ),
            (
                "findings, world_hash",
                "clipping/masking suggestions and simulation result",
            ),
        ],
        writes: "optional mix WAV and stem directory",
        example: "galaxy-gm audio mix mix.toml --out mix.wav --stems stems",
        next: "Compare effective playback level, then tune audio.toml volume or process source gain.",
        topic: "audio",
    },
    Command {
        tool: Tool::Gm,
        path: "audio judge",
        synopsis: "--intent <text> <clip> [--model <id>]",
        answers: "Describe audio blind, then assess its fit to an intent.",
        about: "One use means one run, making two paid calls on success (a failure may stop after one). First is blind; second sees intent. Never retries. Model certainty is unreliable: two runs can disagree; measurements outrank descriptions. Key only from OPENROUTER_API_KEY. At most 120 s. No calls during play.",
        args: &[
            ("--intent <text>", "required intent, 1..4000 bytes"),
            (
                "--model <id>",
                "audio-capable OpenRouter model; default google/gemini-3.8-flash",
            ),
        ],
        output: &[
            (
                "blind_description, assessment",
                "timestamped mood, instruments, energy, fit and uncertainty",
            ),
            (
                "model, calls, cost_usd",
                "actual model, per-request usage and actual cost; missing cost is null",
            ),
        ],
        writes: "",
        example: "galaxy-gm audio judge --intent \"quiet warm background pads\" bed.wav",
        next: "Use inspect and check for numeric acceptance; semantic judgment is fallible.",
        topic: "audio",
    },
    Command {
        tool: Tool::Gm,
        path: "audio check",
        synopsis: "<spec.toml> [--detail]",
        answers: "Check numeric audio expectations with actionable failures.",
        about: "TOML has clip or scenario (exactly one), paths relative to the spec, and [[expect]] with value (dotted report path), at_least/at_most or equals. For source seams set looping=true. Unknown fields are input errors; undefined numbers fail. Never calls a model. --detail includes windows.",
        args: &[(
            "--detail",
            "include every measurement window and individual finding",
        )],
        output: &[(
            "ok, expectations, findings, run",
            "bounds, got, pass; suggested_action only on failures; local summary once",
        )],
        writes: "",
        example: "galaxy-gm audio check bed-check.toml",
        next: "Exit 5 audio_expectations means repair the named values and check again.",
        topic: "audio",
    },
    Command {
        tool: Tool::Gm,
        path: "live status",
        synopsis: "[--game <pid|session>]",
        answers: "What is the running game doing: its build, modules, tick, world hash, the \
                  player's place and the frame rate?",
        about: "Reads never change the world: the hash a read reports is the hash the session \
                records. Without a running game it exits 2 with kind no_game; with several \
                and none clearly meant, kind several_games and the list.",
        args: &[(
            "--game <pid|session>",
            "which running game, by its pid or session folder name; needed only when \
             several run and none is clearly meant (the error lists them)",
        )],
        output: &[
            (
                "game",
                "the game read: pid, session, build and why it was chosen",
            ),
            (
                "modules",
                "each running module: id, version and the folder it was loaded from",
            ),
            (
                "tick, seconds, hash",
                "the world's tick (30 a second), the same in seconds, and \
              its hash",
            ),
            (
                "place",
                "the player being and the place it is in; null without one",
            ),
            (
                "fps, frames",
                "frames a second over about the last second, and frames drawn",
            ),
            (
                "window",
                "logical size and scale; null before the window opens",
            ),
            (
                "paused, paused_why",
                "true while an F8 note is open: the world waits",
            ),
            ("entities, facts, hot_loads", "counts so far"),
            (
                "session_dir, notes, watching_folders",
                "the session folder, notes kept, and \
              whether play runs with --watch",
            ),
        ],
        writes: "",
        example: "galaxy-gm live status",
        next: "galaxy-gm live state for the world, live history for what just happened.",
        topic: "live",
    },
    Command {
        tool: Tool::Gm,
        path: "live state",
        synopsis: "[--entity <id>] [--component <name>] [--all] [--game <pid|session>]",
        answers: "What is in the running game's world: a line an entity, or one entity's or \
                  component's fields?",
        about: "The same read as galaxy sim run --state, on the world as it is now. An ID may \
                be bare (walker for demo.walker) when one module has it.",
        args: &[
            (
                "--entity <id>",
                "only this entity, every component's fields",
            ),
            (
                "--component <name>",
                "only this component, on every entity that has it",
            ),
            ("--all", "every entity's fields, every fact and every cause"),
            (
                "--game <pid|session>",
                "which running game, by its pid or session folder name; needed only when \
             several run and none is clearly meant (the error lists them)",
            ),
        ],
        output: &[
            ("tick, hash", "when it was read"),
            (
                "state.entities",
                "a line each (where it is, speed, model, components), or with \
              --entity/--component their fields",
            ),
            (
                "state.facts",
                "facts counted by kind (or all of them with --all)",
            ),
        ],
        writes: "",
        example: "galaxy-gm live state --entity walker",
        next: "galaxy-gm live watch to see a value change over ticks.",
        topic: "live",
    },
    Command {
        tool: Tool::Gm,
        path: "live history",
        synopsis: "[--last <n>] [--since <tick>] [--kind <text>] [--inputs] [--game <pid|session>]",
        answers: "What just happened in the running game: the latest facts (inputs among them), \
                  who made each, and what scripts printed?",
        about: "A fact is an event something recorded: a key or click (made by an input), a \
                script's ctx.record, an engine service. Oldest first.",
        args: &[
            (
                "--last <n>",
                "the latest n matching facts, 1 to 500; 20 by default",
            ),
            ("--since <tick>", "only facts from this tick on"),
            ("--kind <text>", "only facts whose kind contains this text"),
            (
                "--inputs",
                "only facts inputs made: keys, clicks, bot steps",
            ),
            (
                "--game <pid|session>",
                "which running game, by its pid or session folder name; needed only when \
             several run and none is clearly meant (the error lists them)",
            ),
        ],
        output: &[
            ("tick", "the world's tick now"),
            (
                "matching, shown",
                "how many facts match, and how many are shown",
            ),
            (
                "facts",
                "each: seq, tick, kind, by (input from keyboard, script <module> <file> \
              on <event>, engine <service>, start of <module>, hot-load of <module>) and \
              payload",
            ),
            (
                "log",
                "lines scripts printed since the first fact shown: tick, module, text",
            ),
        ],
        writes: "",
        example: "galaxy-gm live history --inputs --last 10",
        next: "galaxy-gm live state --entity <id> for what a fact changed.",
        topic: "live",
    },
    Command {
        tool: Tool::Gm,
        path: "live watch",
        synopsis: "--entity <id> --component <name> [--field <name>] [--ticks <n>] [--every <n>] [--game <pid|session>]",
        answers: "How does a value in the running game change over the next ticks?",
        about: "Samples now and after each tick the game runs, then prints them all at once \
                when the ticks have passed (about ticks / 30 seconds). A paused game (an open \
                F8 note) holds the watch; the command gives up after twice real time.",
        args: &[
            ("--entity <id>", "the entity; required"),
            ("--component <name>", "its component; required"),
            (
                "--field <name>",
                "one field of it; the whole component without",
            ),
            (
                "--ticks <n>",
                "how many ticks to watch, up to 9000; 30 (one second) by default",
            ),
            (
                "--every <n>",
                "sample every n ticks; 1 by default; at most 1000 samples",
            ),
            (
                "--game <pid|session>",
                "which running game, by its pid or session folder name; needed only when \
             several run and none is clearly meant (the error lists them)",
            ),
        ],
        output: &[
            ("entity, component, field", "what was watched"),
            ("from_tick, to_tick", "the first and last tick sampled"),
            (
                "samples",
                "each tick and value (null once the component is gone)",
            ),
            (
                "first, last, changes",
                "the first and last value, and how often it changed \
              between samples",
            ),
            (
                "range",
                "min and max when every value is a number; else null",
            ),
        ],
        writes: "",
        example: "galaxy-gm live watch --entity walker --component engine.position --field x --ticks 60",
        next: "If it moves wrongly, galaxy-gm live history shows what moved it; film it with \
               galaxy-gm film <session>/recording.json after closing the game.",
        topic: "live",
    },
    Command {
        tool: Tool::Gm,
        path: "live shot",
        synopsis: "[--out <file.png>] [--game <pid|session>]",
        answers: "What does the running game's window show right now? Writes it as a PNG.",
        about: "The frame is drawn again at the moment the window last drew, at the window's \
                size and scale, without the F8 note box.",
        args: &[
            (
                "--out <file.png>",
                "where the picture goes; shots/tick-<n>.png in the game's \
              session folder by default",
            ),
            (
                "--game <pid|session>",
                "which running game, by its pid or session folder name; needed only when \
             several run and none is clearly meant (the error lists them)",
            ),
        ],
        output: &[
            ("out", "the picture's path"),
            ("size", "its width and height in pixels"),
            ("tick, hash", "the world when it was drawn"),
        ],
        writes: "the PNG at --out, or <session>/shots/tick-<n>.png",
        example: "galaxy-gm live shot --out now.png",
        next: "Look at the picture; galaxy-gm live state says where things are.",
        topic: "live",
    },
    Command {
        tool: Tool::Gm,
        path: "live load",
        synopsis: "[<module>] [--game <pid|session>]",
        answers: "Load changed modules into the running game without restarting it.",
        about: "Without a folder it reloads each running module whose files changed in the \
                folder it was loaded from; with a module folder (or a folder of modules) it \
                loads those, changed or not (a module folder elsewhere replaces the running \
                module of its ID). It is a hot-load like play --watch: between two ticks, \
                checked first (the scripts load, the world still fits; else nothing changes \
                and it exits 5 with kind hot_load_refused), recorded with the new files, so \
                the session still replays, audits and films. Scripts keep no state, so the \
                world carries on; a changed start.toml does not move things already placed. \
                A style.toml-only change is presentation only. While an F8 note is open it \
                is refused (kind hot_load_refused): keep or drop the note, then load.",
        args: &[
            (
                "<module>",
                "a module folder or a folder of modules; the running ones by default",
            ),
            (
                "--game <pid|session>",
                "which running game, by its pid or session folder name; needed only when \
             several run and none is clearly meant (the error lists them)",
            ),
        ],
        output: &[
            (
                "loaded",
                "each module loaded: module, version, dir and presentation_only",
            ),
            (
                "unchanged",
                "running modules whose files had not changed (skipped)",
            ),
            (
                "failed",
                "each module refused: module and error; nothing of it changed",
            ),
            (
                "tick, hash",
                "the world after the load; the load runs before the next tick",
            ),
            (
                "recording",
                "the session's recording, written with the load in it",
            ),
        ],
        writes: "the session's recording.json",
        example: "galaxy-gm live load",
        next: "galaxy-gm live watch or live state to see the change take; galaxy sim replay \
               <session>/recording.json after closing the game proves it replays.",
        topic: "live",
    },
    Command {
        tool: Tool::Gm,
        path: "look",
        synopsis: "<recording> (--note <n> | --tick <n>) [--point <x,y>] [--size <w>x<h>]",
        answers: "What is under a point of a recorded frame; is its hover readable?",
        about: "Replays embedded modules headlessly. Points and rectangles use logical pixels. Notes use their exact recorded frame; tick mode uses alpha 1 and the requested viewport.",
        args: &[
            ("<recording>", "recording.json or its session folder"),
            (
                "--note <n>",
                "F8 note number, starting at 1; exclusive with --tick",
            ),
            ("--tick <n>", "recorded simulation tick, drawn at alpha 1"),
            (
                "--point <x,y>",
                "logical pixel coordinates; defaults to the note pointer",
            ),
            (
                "--size <w>x<h>",
                "tick-mode logical viewport, default 1280x720; notes retain their size",
            ),
        ],
        output: &[
            (
                "widget, entity, being, part, place, anchor",
                "topmost widget path and underlying anchor or nearest mesh part; null where absent",
            ),
            (
                "hover",
                "text, rectangle, readable and problems; null means no authored hover",
            ),
        ],
        writes: "",
        example: "galaxy-gm look sessions/example/recording.json --note 3",
        next: "Compare the point with the note frame; galaxy-gm guide hovers explains declarations and checks.",
        topic: "hovers",
    },
    Command {
        tool: Tool::Gm,
        path: "guide",
        synopsis: "[topic]",
        answers: "What do you want to do, and which command does it? For both galaxy and \
                  galaxy-gm, with an example each.",
        about: "",
        args: &[(
            "[topic]",
            "play, live, notes, replay, tests, motion, screens, rooms, talk, life, hovers, audio, modules, models, style or install; without one, every task \
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
        path: "test",
        synopsis: "<module> [--only <text>]",
        answers: "Run Luau tests, scenarios and declared rendered bots; report each result.",
        about: "A Luau test file returns a table of named functions; one passes if it returns \
                without an error (assert(cond, \"why\") fails it). Tests run in the scripts' \
                sandbox and budgets, require(\"../scripts/name\") sees the module's files, and \
                there is no ctx: they test pure Luau. A scenario run is a seed, ticks, \
                [[input]] entries as in an inputs file (a plain name is the module's own event) \
                and [[expect]] entries on component fields or fact counts after a tick; script \
                failures during it fail it too. The modules beside it are loaded, as for check. \
                tests/*.bot.toml explicitly opens a window: inputs (module-relative file), ticks, \
                size = [1280, 720] (physical pixels), optional seed (default 1). No resize or hot_load. \
                It runs the matching galaxy play and replays its recording. Missing GPU/window is \
                status unavailable, never a pass. Any failure exits 5 with kind tests_failed.",
        args: &[
            ("<module>", "a module folder (with a manifest.toml)"),
            (
                "--only <text>",
                "run only the tests whose file or name contains this text",
            ),
        ],
        output: &[
            ("ok", "every test passed"),
            (
                "module, version, context_modules",
                "the module tested and the modules beside it",
            ),
            (
                "passed, failed",
                "how many tests, scenarios and bots passed and failed (each run counts once)",
            ),
            (
                "failures",
                "a line for each failure: the file, the test and what failed (with its line, or \
                 the tick and the value found against the one expected)",
            ),
            (
                "tests",
                "each Luau test: file, name ((load) when the file itself fails), passed, failure",
            ),
            (
                "scenarios",
                "each scenario run: file, passed, ticks, the world's hash at the end, failures",
            ),
            (
                "bots",
                "file, passed, status, requested_size, size, frames, frame_time (fps and interval_ms median/p95/p99/max), replay.identical, session; failure explains any unavailable or failed run",
            ),
            (
                "notes",
                "modules beside it that could not be read; no tests found",
            ),
        ],
        writes: "declared bots keep a play session with recording, save and note pictures",
        example: "galaxy-gm test game/modules/core",
        next: "Fix the module or the test; galaxy-gm fields inputs lists the inputs a \
               scenario can send, galaxy-gm guide tests shows all formats.",
        topic: "tests",
    },
    Command {
        tool: Tool::Gm,
        path: "check",
        synopsis: "<module>",
        answers: "Does this module load? Its manifest, start state, scripts, models and \
                  style, screens (and those its buttons open) and every room at 1280x720, audio files, and whether what its start state wears exists.",
        about: "The modules beside it are loaded too, since a module may use another's types \
                and events. Screen functions run after engine.start; readability findings name widget paths. Every room is entered in a cloned world to check its beings against the camera and panel. Every reachable widget and anchored thing is hovered, including scrolled content; hovers reports the point, identity, label and readability. Every talk line is composed with its place and screens; speaking focus parts are checked against viewport, painted widgets and scene geometry, including active walk poses. From the first tick and with the player in each place, check presses every visible button and ui.target, two presses deep (at most 32 new states per start, told apart by the screen and by what event handlers changed), running the handlers and landing trips, and checks each new state's screens and name signs with talk silent; pressed lists the states per start. Findings exit 5 with kind findings.",
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
                "each problem: kind (schema, script, start, look, screen, press, hover, room, talk, life or sound) and message",
            ),
            ("notes", "modules beside it that could not be read, skipped"),
            (
                "hovers",
                "reachable targets in staged places and talk lines: widget, entity, part, point, scroll and hover text/readability; null means no declared label",
            ),
            (
                "pressed",
                "per start (null: the first tick, else a place): new states reached by presses, and truncated when the cap stopped it",
            ),
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
        synopsis: "[style | components | manifest | inputs | spec | recipe] [--component <name>] \
                   [--modules <dir>]",
        answers: "Which keys and fields exist, with their types, defaults and ranges? For \
                  style.toml, the engine's components (and a module's own), manifest.toml, \
                  inputs files, motion check specs and model recipes (every shape and its \
                  keys).",
        about: "Generated from the engine's own definitions, so it is what this build \
                accepts. Without a topic, a one-line summary of each.",
        args: &[
            (
                "[topic]",
                "style, components, manifest, inputs, spec or recipe",
            ),
            (
                "--component <name>",
                "components only: just this one, by full name or bare (flight for \
              engine.flight)",
            ),
            (
                "--modules <dir>",
                "components, style or inputs. With components: also the components these \
              modules declare. With style: also their looks and models, and which things \
              wear each (`worn`). With inputs: the inputs they take and the keys that send \
              each (`inputs`)",
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
                  its outline as collision sees it, its morph target, which parts glow or \
                  carry a bite, and its markers (camera, spot.<name>), without reading the \
                  glTF file.",
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
                "without a model: each one's file, parts, markers, triangles, size and who \
              wears it",
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
                "the parts that glow (self-lit, from glowing paint such as lights) or carry a \
              bite (paint marked as a fresh cut, kept out of the shadow hatching), and how much",
            ),
            (
                "markers",
                "each named node without a mesh, by name (camera, spot.<name>): at (its \
              place), looks (the way it faces; a camera looks along it) and up, in the \
              model's frame; a room's camera and its spots stand at these",
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
        path: "model make",
        synopsis: "<module> [<recipe>] [--check]",
        answers: "Make a module's models from its recipes: recipes/<name>.toml becomes \
                  models/<name>.gltf, with the same bytes on every OS.",
        about: "A recipe lists parts, each a shape (tiers, spoked_ring, truss, body, wing, \
                nozzle, rock and more; galaxy-gm fields recipe lists them and their keys) or \
                another recipe used as a preset, placed, repeated and painted from \
                recipes/palette.toml. Each part is one glTF node, so one part for ink and for \
                galaxy-gm model; markers are empty named nodes (camera, spot.<name>). All or \
                nothing: a bad recipe is refused with its file and field (exit 4, fields \
                recipe and field) and nothing is written. The same recipe and seed give the \
                same file; change the seed for a variant.",
        args: &[
            ("<module>", "a module folder (with a manifest.toml)"),
            (
                "[recipe]",
                "one recipe, by name (outpost for recipes/outpost.toml); every recipe by default",
            ),
            (
                "--check",
                "write nothing; exit 5 (kind findings) when a model is missing or is not what \
              its recipe makes",
            ),
        ],
        output: &[
            (
                "made",
                "each recipe: recipe, model (its ID), file, status (written, unchanged, or with \
              --check stale or missing), parts, markers, triangles, size and sha256 of the file",
            ),
            ("ok", "false with --check when a model is stale or missing"),
        ],
        writes: "models/<name>.gltf in the module, for each recipe made (unless --check)",
        example: "galaxy-gm model make tests/play/yard",
        next: "galaxy-gm model <module> <model> lists its parts; galaxy shot --modules <folder> \
               or galaxy play draws it; galaxy-gm check <module> checks what wears it.",
        topic: "models",
    },
    Command {
        tool: Tool::Gm,
        path: "music",
        synopsis: "<module> --name <sound> --prompt <text> [--event <kind>] [options]",
        answers: "Make a module's music with one OpenRouter generation. It is written into the \
                 module as an audio file and declared in its audio.toml, so the game plays it.",
        about: "Needs OPENROUTER_API_KEY in the environment; it is read from there only, sent \
                 only as the request's authorization, and never printed or written. Without it, \
                 exit 2 with kind no_key and nothing is written. Every run makes one paid call \
                 (about $0.04 for the default model) and is never retried by itself, so run it \
                 once per piece and commit the file. The game never calls a model; music is made \
                 only here, at build time. The prompt is sent with \"Instrumental only\" added. \
                 The answer (MP3 for the default model) is checked to decode before anything is \
                 written. It becomes music/<name>.<ext> and [sounds.<name>] with a comment naming \
                 the model and prompt; --event adds an [[events]] cue on the music bus, which \
                 loops it and crossfades from other music. Its volume comes from the clip's \
                 measured loudness, so it sits in the background. Describe the timbre as well \
                 as the mood: nothing is added to soften it. Usage, module and overwrite problems \
                 are reported before the key is needed or any call is made. A refused call, no \
                 answer or unplayable audio exits 1 with kind openrouter and writes nothing.",
        args: &[
            ("<module>", "a module folder (with a manifest.toml)"),
            (
                "--name <sound>",
                "the sound's local name: lowercase letters, digits and _",
            ),
            (
                "--prompt <text>",
                "what the music should sound like: mood, tempo, instruments and timbre \
                 (\"soft, mellow, rounded, no bright or harsh highs\"); up to 1000 characters",
            ),
            (
                "--event <kind>",
                "also cue it on the music bus when this event happens: a local event the \
                 manifest declares, or a full kind such as engine.start; default: no cue",
            ),
            (
                "--volume <0..1>",
                "the sound's volume; default: the background level (volume times the clip's \
                 RMS is 0.0316, -30 dBFS), or with --replace the loudness the old clip had",
            ),
            (
                "--fade <s>",
                "the cue's fade-in in seconds, 0..60; default 1",
            ),
            (
                "--model <id>",
                "an OpenRouter model with audio output; default google/lyria-3-clip-preview \
                 (about 30 s); google/lyria-3-pro-preview makes a longer piece",
            ),
            (
                "--seed <n>",
                "passed to the model, which may or may not honour it",
            ),
            (
                "--replace",
                "generate again over an existing sound or music/<name> file; without it that \
                 is bad_input",
            ),
        ],
        output: &[
            (
                "ok",
                "the module's sounds have no findings after the change",
            ),
            (
                "module, sound, file",
                "where the music went: file is relative to the module",
            ),
            ("format, bytes, seconds, sample_rate", "the decoded audio"),
            (
                "volume, rms",
                "the volume written, and the decoded clip's RMS it was chosen from",
            ),
            ("event", "the cue added, or null"),
            (
                "replaced",
                "the file the sound named before --replace, or null",
            ),
            (
                "model, answered_by, cost_usd, generation_id",
                "the model asked for, the one that answered, and what OpenRouter charged",
            ),
            (
                "prompt, seed, text",
                "what was sent, and any text the model sent back",
            ),
            (
                "findings",
                "sound problems in the module now, as check reports them",
            ),
            ("next", "what to run to hear it"),
        ],
        writes: "<module>/music/<name>.<ext> and <module>/audio.toml (created if missing; \
                 existing comments and entries are kept)",
        example: "galaxy-gm music game/modules/example --name calm --prompt \"slow warm pads, \
                 quiet and unhurried; soft, mellow, rounded, no bright or harsh highs\" \
                 --event engine.start",
        next: "galaxy-gm check <module>, then galaxy play --modules <modules folder> and listen; \
                 galaxy audio renders the mix to a WAV without a device.",
        topic: "audio",
    },
    Command {
        tool: Tool::Gm,
        path: "memory",
        synopsis: "<modules>",
        answers: "What does the GM remember of the player and this game?",
        about: "The player's preferences and the long-running directions not yet in the game, \
                in one short Markdown file a game (the module IDs in <modules>), kept in the \
                per-user game data folder beside the sessions (GALAXY_GM_MEMORY moves it), so \
                it outlives reinstalls and changes the player rolls back. The first run makes \
                it from a template. Read it before choosing work; to update it, edit the file \
                itself. The player may edit it too, and what they write there wins. It never \
                holds secrets: a memory holding an API key is refused (exit 4), unprinted.",
        args: &[(
            "<modules>",
            "the game: a module folder, or a folder of modules",
        )],
        output: &[
            ("game", "the module IDs"),
            ("path", "the memory file, to edit"),
            ("created", "true when this run made it from the template"),
            ("lines, text", "its length and its whole text"),
        ],
        writes: "the memory file, only when it is missing",
        example: "galaxy-gm memory game/modules",
        next: "galaxy-gm notes inbox --modules <modules> for the player's new notes; \
               galaxy-gm guide unattended for a whole run.",
        topic: "unattended",
    },
    Command {
        tool: Tool::Gm,
        path: "notes",
        synopsis: "[<recording | session folder | sessions folder>] [--all] [--modules <dir>]",
        answers: "Read F8 notes; notes inbox lists those still to answer. \
                  Each note has its number, tick, time, text and frame, newest sessions first.",
        about: "Without a path it reads the sessions folder every `galaxy play` records into \
                (`galaxy doctor` names it; GALAXY_SESSIONS moves it). Sessions without notes \
                are counted, not listed. This history includes handled notes; \
                `notes inbox` excludes them.",
        args: &[
            (
                "[path]",
                "a recording, a session folder, or a folder of sessions; the sessions \
              folder by default",
            ),
            ("--all", "every note; 20 at most by default"),
            (
                "--modules <dir>",
                "only sessions that played exactly the modules in <dir> (one game)",
            ),
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
              audited), handled (null or reason and change reference)",
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
        path: "notes inbox",
        synopsis: "[<path>] [--handled] [--all] [--modules <dir>]",
        answers: "List unanswered F8 notes across sessions, newest first.",
        about: "Uses the same folders and note numbers as notes, audit and film. \
                With no path, reads the played sessions folder (GALAXY_SESSIONS moves it). \
                --handled shows answered notes instead. The cap applies after filtering. \
                A fresh install with no sessions returns an empty inbox.",
        args: &[
            (
                "[path]",
                "a recording, session folder or sessions folder; played sessions by default",
            ),
            (
                "--handled",
                "list only handled notes, with the reason and change reference",
            ),
            ("--all", "every matching note; 20 at most by default"),
            (
                "--modules <dir>",
                "only sessions that played exactly the modules in <dir> (one game)",
            ),
        ],
        output: &[
            (
                "sessions",
                "newest sessions first: session, recording, build, modules, ticks, seconds and notes",
            ),
            (
                "sessions.notes",
                "newest notes first: note (number for audit/film), tick, t (seconds), text, seen (frame if recorded), drawn_again (if audited), handled (null or reason and change)",
            ),
            (
                "view, more",
                "unhandled or handled; more says how many matching notes were capped",
            ),
            (
                "unreadable, sessions_without_notes, sessions_of_other_modules",
                "recordings that could not be read, and counts of sessions without notes or left out by --modules, when present",
            ),
        ],
        writes: "",
        example: "galaxy-gm notes inbox",
        next: "Read the frame, audit or film the moment, then notes handle <session> --note <n> --reason <text>.",
        topic: "notes",
    },
    Command {
        tool: Tool::Gm,
        path: "notes handle",
        synopsis: "<recording | session folder> --note <n> --reason <text> [--change <reference>]",
        answers: "Mark a note handled with the change that answered it.",
        about: "The reason is required; the change reference can name a commit, PR or module version. \
                Repeating the command replaces this note's receipt. Receipts live with the session, \
                outside installed builds, and survive new sessions and reinstalls. A receipt is \
                matched to the original note; replacing the recording cannot hide a different note.",
        args: &[
            (
                "<recording | session folder>",
                "the recording or session printed by notes inbox",
            ),
            (
                "--note <n>",
                "the note number, counting from 1, as printed by notes",
            ),
            (
                "--reason <text>",
                "nonempty explanation of the change that answered the note",
            ),
            (
                "--change <reference>",
                "optional nonempty commit, PR or module version; stored as text",
            ),
        ],
        output: &[
            (
                "recording, note, handled",
                "the recording, note number and true",
            ),
            (
                "reason, change, mark",
                "the explanation, optional reference and receipt file",
            ),
        ],
        writes: "<recording filename>.handled/<note padded to 4 digits>.json beside the recording, \
                 for example <session>/recording.json.handled/0001.json; atomically replaced",
        example: "galaxy-gm notes handle <session> --note 1 --reason \"Adjusted the panel spacing\" --change <commit>",
        next: "notes inbox no longer lists it; notes inbox --handled shows the answer; notes unhandle reopens it.",
        topic: "notes",
    },
    Command {
        tool: Tool::Gm,
        path: "notes unhandle",
        synopsis: "<recording | session folder> --note <n>",
        answers: "Reopen a handled note in the inbox.",
        about: "Removes the receipt, leaving the recording and frame untouched. \
                A note already unhandled succeeds too; a nonexistent note is bad input (exit 4).",
        args: &[
            (
                "<recording | session folder>",
                "a recording or session folder",
            ),
            ("--note <n>", "the note number, counting from 1"),
        ],
        output: &[
            (
                "recording, note, handled",
                "the recording, note number and false",
            ),
            ("reason, change, mark", "null after reopening"),
        ],
        writes: "removes this note's file from <recording filename>.handled/ beside the recording",
        example: "galaxy-gm notes unhandle <session> --note 1",
        next: "galaxy-gm notes inbox lists it again.",
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
        path: "what-if",
        synopsis: "<recording> [--all] --modules <dir>",
        answers: "What would a module change have done to this session? Runs the recording's \
                  inputs on the changed modules beside a replay of the recording and reports \
                  the first tick where the world differs, and what differs there.",
        about: "The given modules replace the recorded ones by ID (others are added); the rest \
                stay as recorded. A session that started fresh starts again from the changed \
                start states with the recorded seed; one that started from a save resumes it. \
                Recorded hot-loads of a changed module are skipped. Both runs use this build, \
                so a difference is the change's alone; baseline says whether the replay still \
                matches the recording. A changed run that can't go on (an input it no longer \
                declares) is reported as the difference, with error. It exits 0 whether or not \
                the world differs.",
        args: &[
            (
                "<recording>",
                "a recording, or a session folder (its recording.json is used)",
            ),
            (
                "--modules <dir>",
                "the changed module folder, or a folder of modules (unchanged ones are listed \
                 under same)",
            ),
            (
                "--all",
                "list every difference; without it, 20 of each kind and 8 report lines",
            ),
        ],
        output: &[
            (
                "report",
                "a few lines: the first tick that differs (or that nothing does) and each \
                 difference there",
            ),
            ("differs", "whether the world ever differs"),
            (
                "tick, time",
                "the first tick that differs (0 is the start), and m:ss.s",
            ),
            ("inputs", "the recorded inputs on that tick"),
            (
                "differences.entities",
                "entity, component, field, recorded and changed values; or only_in (recorded or \
                 changed) for an entity or component one run lacks",
            ),
            (
                "differences.facts",
                "facts recorded on that tick in only one run: kind, payload, only_in",
            ),
            (
                "differences.other, differences.more",
                "other state that differs (random streams, causes); how many differences were \
                 left out",
            ),
            (
                "error",
                "why the changed run could not go on, when it failed",
            ),
            (
                "modules",
                "which given modules are changed, added and the same",
            ),
            (
                "start",
                "rebuilt from the changed start states, or resumed from the saved world",
            ),
            (
                "skipped_hot_loads",
                "recorded hot-loads of changed modules, not applied",
            ),
            (
                "baseline",
                "matches_recording and a note: does this build still replay it",
            ),
        ],
        writes: "",
        example: "galaxy-gm what-if sessions/example/recording.json --modules game/modules/core",
        next: "galaxy-gm recording <recording> timeline --from <tick> shows what the player was \
               doing then; galaxy sim run --state --ticks <tick> shows the changed world.",
        topic: "tests",
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
              channel, 0 to 255) and pixels_differing (the share of pixels, 0 to 1), or \
              seen_error when the seen frame can't be read",
            ),
            (
                "ok",
                "true when every note's world matches; it says nothing about pictures, \
              which pictures_compared and pictures report",
            ),
            (
                "pictures_compared, pictures",
                "how many note pictures were compared with the frames the player saw, and \
              in words why not all of them (a recording moved out of its session folder has \
              no notes/ beside it, so only the world hashes were checked)",
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
        answers: "Film a played session round a note or between two ticks, frame by frame \
                  as the window showed it.",
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
                checked against the world's hash it kept, and a film to the recording's last \
                tick against the world it ended on (a live load just before the end included), \
                so a film of a different world exits 5 (replay_diverged) rather than pass for what the player saw. By default the \
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
                "end_checked",
                "when the film ends on the recording's last tick: tick, identical \
              (the film ends on the world the recording ended on); null otherwise",
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
                "runs",
                "each tool run once for the same options: run (its number), tool, \
              options, its reading and picture (or the error)",
            ),
            (
                "expectations",
                "one short line each: run (the run it read), tool, value, the bounds, got \
              (the value measured), pass",
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
        answers: "Which engine and builds are installed, and which modules are nearby?",
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
        name: "unattended",
        title: "Unattended GM runs: from the player's notes to a reviewable change",
        about: "A scheduled run with nobody watching: nobody answers questions, so decide, and \
                put what needs the player in the handover. The game is the modules in the \
                working folder (galaxy-gm doctor names them, and the build to use); its own \
                instructions (AGENTS.md, README) come first. Never build the engine. The memory \
                carries the player's preferences and long-running directions between runs; the \
                notes are the player's new requests, each with the moment it was taken. The \
                change goes on a git branch the player merges (accept) or deletes (roll back), \
                and their game carries on from its save: galaxy play --continue.",
        tasks: &[
            ("Your memory of the player and the game", "gm memory"),
            ("The player's new notes for this game", "gm notes inbox"),
            (
                "Does the player's save load on the change?",
                "game sim load",
            ),
        ],
        recipes: &[Recipe {
            title: "One unattended run, start to handover",
            steps: &[
                "Read first: the game's own instructions, galaxy-gm doctor (the build and the game's modules), then:",
                "galaxy-gm memory <modules>",
                "galaxy-gm notes inbox --modules <modules> --all",
                "galaxy sim load --continue --modules <modules> --state",
                "Settle earlier handovers in the memory: a branch the player merged is done; one they deleted is a rollback: note what they turned down as a preference and reopen its notes (galaxy-gm notes unhandle) unless they said otherwise.",
                "Understand each note from its moment: its text, the frame it saw (seen), then:",
                "galaxy-gm audit <session>",
                "galaxy-gm film <session> --note <n> --before 2 --after 2",
                "Pick work: the notes, oldest first, then one step of a direction in the memory. A small, clear request done fully beats a big one half done. A note that needs the player's decision stays unhandled; ask in the handover.",
                "Work in a new git worktree on a branch gm/<date> from the default branch, so the player's files stay as they are until they accept (not under git: git init and commit the folder as it is first). Follow the topic guides for the edits. Your own plays set GALAXY_SESSIONS to a scratch folder, so their notes and saves stay out of the player's.",
                "The gate: every step passes before the handover. galaxy-gm check and galaxy-gm test on each changed module, then:",
                "galaxy-gm what-if <session>/recording.json --modules <changed-modules>",
                "galaxy play --modules <changed-modules> --inputs <bot.inputs> --ticks <n>",
                "galaxy sim replay <bot-session>/recording.json",
                "Look at each changed moment (galaxy shot, or a film and a motion tool for what moves). Then, with the player's sessions folder:",
                "galaxy sim load --continue --modules <changed-modules>",
                "It must load: the player's game carries on. Exit 4 names what no longer fits; give new fields defaults and keep the fields the save holds.",
                "Commit one change per note, its message naming the note, what changed and the gate's evidence. Then mark each answered note:",
                "galaxy-gm notes handle <session> --note <n> --reason <text> --change gm/<date>",
                "Hand over the branch. Push it and open a pull request only if the game's instructions say so; never merge. Report what changed and why, the evidence, how to accept (merge, then galaxy play --modules <modules> --continue) or roll back (delete the branch), and what you need from the player.",
                "Last, edit the memory file: preferences learned, directions still open, this handover as waiting. Short, nothing the game already says, never a secret.",
            ],
        }],
    },
    Topic {
        name: "tests",
        title: "Module tests and what a change does to a session",
        about: "A module's tests live in its tests/ folder. tests/<name>.luau returns { \
                name = function() ... end }: each function is a test that passes if it returns \
                without an error, so write assert(rules.reached_deck(at), \"1.9 high reaches \
                the deck\"). It runs in the scripts' sandbox, can require(\"../scripts/rules\") \
                and has no ctx, so keep the rules you want to test in plain functions. \
                tests/<name>.toml is a scenario run: seed, ticks, [[input]] entries (tick, \
                name, source, payload; a plain name is the module's own event, payload IDs are \
                full, and an input on tick N has run when tick N's expectations are checked) \
                and [[expect]] entries checked after their tick (the last by default): entity \
                (the module's own name, or a full ID), component and field with equals, \
                at_least or at_most (absent = true without a field), or facts = \"<event>\" \
                with a count. tests/<name>.bot.toml declares a rendered run: inputs = \
                \"tests/run.inputs\", ticks = 300, size = [1280, 720], optional seed = 1 (one key per line). \
                Inputs are module-relative, use full event IDs, and cannot resize or hot-load. \
                galaxy-gm test opens a window only for declared bots; --only filters their file names. \
                bots reports physical size, frames, fps, interval_ms median/p95/p99/max and replay.identical. \
                Missing GPU/window reports status unavailable and exits 5, never a pass. \
                galaxy-gm what-if \
                runs a recorded session (galaxy play, or galaxy sim run --record) again on \
                changed modules and names the first tick where the world differs and the \
                entities, fields and facts that differ.",
        tasks: &[
            ("Run a module's tests and scenario runs", "gm test"),
            (
                "What a module change does to a recorded session: the first tick that differs",
                "gm what-if",
            ),
        ],
        recipes: &[Recipe {
            title: "Add a test, run it, then see which tick a script change first alters",
            steps: &[
                "Write tests/rules.luau in the module: local rules = require(\"../scripts/rules\"), then return { steps_add_up = function() assert(rules.STEP == 1, \"one step a second\") end }.",
                "galaxy-gm test <module> --only rules",
                "Record a session by playing it (galaxy play --modules <modules>), or script one:",
                "galaxy sim run --modules <modules> --seed 7 --ticks 450 --inputs <inputs.toml> --record session.json",
                "Copy the module, change a value in one of its scripts, then:",
                "galaxy-gm what-if session.json --modules <changed module>",
                "Read report: the first tick that differs, and the fields and facts that differ there.",
            ],
        }],
    },
    Topic {
        name: "live",
        title: "Reading and changing a running game",
        about: "galaxy play serves a live socket while it runs (local only: a named pipe on \
                Windows, a Unix socket elsewhere, open to this user, with a token; --no-live \
                turns it off). galaxy-gm live reads the game through it: status, state, \
                history, watch, shot. Reads never change the world or its hash. live load \
                hot-loads changed modules between two ticks and records it like play --watch, \
                so the session still replays. Each command picks the one running game, or the \
                one running the modules it loads, or the one playing modules in the working \
                folder; otherwise it lists them (kind several_games) and --game <pid or \
                session> picks one. With no game running, kind no_game.",
        tasks: &[
            ("Tick, place, frame rate", "gm live status"),
            ("Where things are, their fields", "gm live state"),
            ("What just happened", "gm live history"),
            ("A value over ticks", "gm live watch"),
            ("What the window shows", "gm live shot"),
            ("Load changed modules without restarting", "gm live load"),
        ],
        recipes: &[Recipe {
            title: "Edit a running game",
            steps: &[
                "galaxy play --modules <modules>",
                "galaxy-gm live status",
                "galaxy-gm live state --entity <being>",
                "Edit the module's script (a different spot to walk to), then:",
                "galaxy-gm live load",
                "galaxy-gm live watch --entity <being> --component engine.position --field x --ticks 90",
                "galaxy-gm live shot --out now.png",
                "Close the window, then: galaxy sim replay <session>/recording.json",
            ],
        }],
    },
    Topic {
        name: "hovers",
        title: "Hover labels and looking at recorded points",
        about: "Every widget accepts hover = \"label text\". ui.target { id, anchor, on, payload, text } makes the anchored model clickable: a click sends on with payload and source ui, like a button; a painted widget in front blocks it; while hovered the model lifts, gets a [screen] highlight outline and the pointer becomes a hand; text is its optional label. Bot [[click]] and [[pointer]] widget = the target id pick a visible point on its model. A pure screen can also return ui.hover { id = \"person-help\", anchor = \"being.demo.person\", text = \"A moving speaker\" } beside ordinary widgets. This declares a label without drawing a permanent widget: the nearest visible model surface activates it, or a 44px anchor target for things without models. A part anchor such as being.demo.person.face restricts it to that mesh part (empty nodes use an anchor target); publisher#name binds an ordinary named anchor. Keep declarations conditional on the current place, like other anchored screens. Later declarations win. The engine shows the label immediately, keeps it inside the viewport, clears it on pointer exit or capture, and follows interpolated anchors. Its text and surface use [screen] text and paper. Hover paint never changes world state. Check stages screens, places and talk lines and reports hovers, including null for targets without a declaration, plus readability problems. It checks authored states, not arbitrary future script branches. Look uses the same picking and layout headlessly. A note preserves its exact pointer, size, style and fractional pose; use --note to match its saved frame. Tick mode uses alpha 1 and --size (default 1280x720). Coordinates are logical pixels; divide physical image coordinates by the note scale.",
        tasks: &[
            ("Inspect a note under its recorded pointer", "gm look"),
            ("Check authored hover labels", "gm check"),
        ],
        recipes: &[Recipe {
            title: "Record a hover and inspect it",
            steps: &[
                "Add hover = \"Add one\" to a button, or ui.hover with an anchor and text to a screen.",
                "galaxy-gm check <module>",
                "galaxy play --modules <modules>; point at the widget or being and press F8.",
                "galaxy-gm look <session>/recording.json --note 1",
                "galaxy-gm look <session>/recording.json --note 1 --point 320,240",
                "galaxy-gm audit <session>/recording.json",
                "galaxy-gm film <session>/recording.json --note 1 --before 1 --after 1",
                "Bot [[pointer]] action = \"move\" needs no active drag. widget names a widget ID, an anchored hover ID, or point:x,y for coordinates. Repeat moves to track a moving target; moving off it clears the label.",
            ],
        }],
    },
    Topic {
        name: "life",
        title: "A recorded day per being, with visible walks and signs",
        about: "Add engine.life to a non-player being with a position: clock names an engine.clock, first names an engine.act, name supplies its sign, speed is world units per second. Each act has positive integer duration, place, spot, state, morph (0..1 in the module's glTF target), angle (radians), and next; empty next repeats from first. The engine uses no activity names or game-specific poses. A visible change rises to neutral morph 0 for 9 ticks, walks, then settles for 9 ticks. Every room used by a cross-room day needs an arrival spot on the act's floor; departures use a matching door spot when present, otherwise the room arrival. Elsewhere changes finish at once. Leaving mid-walk or changing life.presentation from states to instant finishes the destination in world state. Scripts alone advance clocks using ctx.advance_clock; each increase, including direct clock writes, records engine.clock_advanced with entity, amount and value. Signs share screen paint. Speaking signs join the bubble; other signs show name and resting activity. check exercises every distinct clock arrangement and transitions between them, with simultaneous beings and every dialogue line at tick and half-tick poses. It reports excessive check spaces explicitly rather than passing an incomplete check. Arbitrary script branches still need their own recorded runs.",
        tasks: &[
            ("Life, act and clock fields", "gm fields"),
            ("Check poses and moving speaker focus", "gm check"),
        ],
        recipes: &[Recipe {
            title: "Try a day, leave during a walk, and reproduce it",
            steps: &[
                "galaxy-gm check engine/tests/play/life/demo",
                "galaxy play --modules engine/tests/play/life --inputs engine/tests/play/life-inputs.toml --ticks 380 --size 1280x720",
                "In live play, click Being A to talk, then Advance clock. Watch the figure rise and its bubble follow it. Leave before it reaches the other spot and return: the act has finished. Advance again and switch Presentation during departure; the hall sign reports the completed destination. Select a dev build with --build <name> immediately after each executable.",
                "galaxy sim replay <session>/recording.json",
                "galaxy-gm audit <session>/recording.json",
                "galaxy-gm film <session>/recording.json --from 10 --to 125 --size 1280x720 --out life-film",
                "galaxy-gm motion track life-film --thing life.walker",
                "galaxy-gm motion sheet life-film --count 12",
                "galaxy-gm guide motion",
            ],
        }],
    },
    Topic {
        name: "audio",
        title: "Module sound, music and place ambience",
        about: "audio.toml declares [sounds.NAME] with file and volume (0..1); WAV, Ogg Vorbis or MP3. [[events]] uses event, sound, bus (sound one-shot or music loop) and fade seconds. [places] maps place IDs to ambience sounds. crossfade defaults to 1 s. Per-sound loop_crossfade defaults to 2 s, disabled for clips shorter than four fades; use 0 for prepared loops. Stop a bus with stop=true and no sound. Cues follow recorded facts; use a separate event for key-down cues. A resumed save replays no earlier cue: an on['engine.resumed'] handler records the music event again. Music replaces music; player location chooses ambience. Audio stays outside world hashes. Binary assets remain on disk; no device means silent play with a note. The mixer owns 32 voices, ramps and resampling. Numeric tools run locally; only judge calls a model.",
        tasks: &[
            ("Generate with OpenRouter", "gm music"),
            ("Audio authoring tools", "gm audio"),
            ("Render playback PCM", "game audio"),
            ("Levels, spectrum, defects and seam", "gm audio inspect"),
            (
                "Gain, trim, fades, DC, clicks and gentle EQ",
                "gm audio process",
            ),
            ("Search and blend a loop", "gm audio loop"),
            ("Actual simultaneous bus levels", "gm audio mix"),
            ("Blind description, then semantic fit", "gm audio judge"),
            ("Numeric acceptance", "gm audio check"),
        ],
        recipes: &[Recipe {
            title: "A quiet, smooth background bed",
            steps: &[
                "Prompt for warm rounded pads, sparse voicing, steady energy, compatible beginning/end, no intro, outro or final cadence. Generate once with galaxy-gm music <module> --name calm --prompt <text> --event engine.start.",
                "galaxy-gm audio inspect clip.mp3 --reference approved.wav. For sharp/harsh complaints locate narrow_resonances (level_dbfs), band energies and attacks/crest against a matched reference before choosing notch/shelf frequencies. Warm low mids are not a defect.",
                "galaxy-gm audio process clip.mp3 --out soft.wav --target-lufs -23. Apply DC/click repair or EQ only for a measured, unwanted problem.",
                "galaxy-gm audio loop soft.wav --out bed.wav --blend 2 --search 1. Set its audio.toml file to bed.wav and loop_crossfade=0. Listen to repetitions.",
                "galaxy-gm audio inspect <module> --sound <name> measures the seam as played. For a standalone file use --loop to measure its hard wrap. Default output is a summary; --detail includes all windows.",
                "Write mix.toml: modules='modules' and ticks=1800 (60 s), optional inputs='route.toml' and seed=1. Paths are relative to the scenario. galaxy-gm audio mix mix.toml --out mix.wav measures sources separately from playback buses.",
                "There is no universal background target. Try music's rendered contribution near -26 LUFS, then tune against foreground effects and ambience. volume and default master 0.7 lower source loudness. A peak ceiling can prevent reaching a target; read gain.peak_limited and after.",
                "galaxy-gm audio judge --intent 'quiet warm background pads' bed.wav makes two paid calls with OPENROUTER_API_KEY. One use/run makes two calls. Two runs can disagree despite certainty claims; measurements outrank the description. Reports usage and actual cost.",
                "Write check.toml: clip='bed.wav', looping=true, then [[expect]], value='loudness.integrated_lufs', at_least=-24.0, at_most=-22.0. Add another [[expect]] with value='seam.level_jump_db', at_most=3.0. galaxy-gm audio check check.toml; exit 5 names failures and repairs. Use scenario='mix.toml' instead of clip to check buses.music.integrated_lufs.",
                "Clipping already baked in needs regeneration: 'clean, undistorted, restrained dynamics'. Stereo cancellation: 'stable centred bass, mono-compatible stereo'. Unwanted muddiness: locate it against a reference; preserve intended warmth. Wrong mood/instruments: change the prompt. Local repairs cannot recover missing musical content.",
            ],
        }],
    },
    Topic {
        name: "play",
        title: "Playing, and taking notes",
        about: "`galaxy play` runs modules in a window. Every session records itself into a \
                folder of its own under the sessions folder: recording.json (everything \
                needed to run it again), save.db (the world at the end) and notes/0001.png. \
                F8 pauses and opens a note box over the frame on screen; Enter keeps the note \
                with its tick, the world's hash and the camera. A bot plays an inputs file \
                through the same window loop (--inputs), and its [[note]] entries are taken \
                like F8 notes. Resume with galaxy play --modules <modules> --load save.db; \
                use the saving build and unchanged module files. --ticks and inputs use absolute \
                world ticks, including on resume. The first tick after a resume (play --load, \
                --continue, sim load) starts with the recorded input engine.resumed (source \
                resume): handle it to cue music or sounds again, since no earlier cue replays. \
                The new recording embeds the saved start world, so replay, F8, audit and film \
                work as usual. --physical-size makes --size physical \
                pixels (otherwise logical); the report always gives physical size. \
                Esc quits and saves unless a module binds escape in [keys]; Shift+Esc and \
                closing the window always do. [keys] can also bind mouse_right; bot \
                [[key]] entries press a key name for one tick.",
        tasks: &[
            ("Play the game", "game play"),
            (
                "Draw one frame headless, or one thing (--crop)",
                "game shot",
            ),
        ],
        recipes: &[],
    },
    Topic {
        name: "notes",
        title: "The player's F8 notes",
        about: "A note is text the player typed, the tick it was on (30 ticks a second), the \
                world's hash then, and the frame that was on screen (notes/0001.png in the \
                session folder). To act on one: read it, see the frame, check the replay \
                reaches the same world (audit draws the moment again into audit/0001.png), \
                then film the seconds round it and measure what moved. Start with notes inbox. \
                After answering a note, handle it with a reason and optional change reference; \
                --handled lists answers, and unhandle reopens a note. Receipts sit beside each \
                recording in recording.json.handled/, so new sessions and engine reinstalls keep them.",
        tasks: &[
            ("Read all notes", "gm notes"),
            ("Find unanswered notes", "gm notes inbox"),
            ("Record an answer", "gm notes handle"),
            ("Reopen a note", "gm notes unhandle"),
            ("Replay and redraw notes", "gm audit"),
            ("Film round a note", "gm film"),
        ],
        recipes: &[],
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
        recipes: &[],
    },
    Topic {
        name: "motion",
        title: "How motion looks",
        about: "A screenshot can't show how something moves. Film a short span frame by frame \
                as the window would draw it (60 fps, at the player's size: thin lines \
                breaking and one-frame pops are missed at 30 fps or small sizes), then ask one \
                question per tool. Each answers with a `reading` sentence, a few numbers, the \
                times (t, in seconds; tick) of suspicious moments and one picture. Probes-only \
                films need no GPU and serve track and dots. A drawn film --note also writes \
                note.png at the exact F8 moment and physical size; numbered frames stay uniformly sampled. \
                When a defect is fixed, keep it \
                fixed with a motion check spec.",
        tasks: &[
            ("Film a scripted run", "game film"),
            (
                "Film a played session round a note; as the player saw it \
                 (--as-recorded), or on two builds (--builds recorded,this)",
                "gm film",
            ),
            ("Where things are, frame by frame", "gm film show"),
            (
                "How far and how fast did a thing move, and when? Film with --probes-only \
                 (no GPU), then track --thing <x>: displacement, path, top speed, overshoot and settling",
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
                "What changed against an approved film, or before a module edit?",
                "gm motion compare",
            ),
            ("Keep a motion expectation as a test", "gm motion check"),
        ],
        recipes: &[
            Recipe {
                title: "Press a key from the start and measure the approach to its target",
                steps: &[
                    "galaxy-gm fields inputs --modules game/modules",
                    "Write keys.toml with the input from that list: [[input]] tick = 1, \
                     name = \"<input>\", source = \"keyboard\", payload = { down = true }",
                    "galaxy film --modules game/modules --inputs keys.toml --from 0 --to 300 \
                     --probes-only --out films/approach",
                    "galaxy-gm motion track films/approach --thing ship --to <target>",
                    "Read approach (top speed, braking_from, stopped_at, arrived_at), gap \
                     (closest, overshoot, rebound, settled_at) and surface (how near the \
                     surfaces came: end, contact, overlap).",
                ],
            },
            Recipe {
                title: "Ram or bump something and measure overlap and bounce",
                steps: &[
                    "galaxy-gm fields inputs --modules game/modules",
                    "Write ram.toml: hold the turn input from tick 1 until the nose points at \
                     the thing (a turn rate of 180 degrees a second is 6 a tick; galaxy-gm \
                     fields components --component flight), let go, and hold the thrust \
                     input from that tick on",
                    "galaxy film --modules game/modules --inputs ram.toml --from 0 --to 240 \
                     --probes-only --out films/ram",
                    "galaxy-gm motion track films/ram --thing ship --to <thing>",
                    "Read surface.overlap (intervals, seconds, deepest, and whether on ticks or \
                     only between them), surface.contact and gap.rebound (how far it bounced \
                     back).",
                    "galaxy-gm film show films/ram --thing ship --gap <thing> --from <t> --to <t>",
                    "That is surface.frames_round_it: the gap frame by frame round the first \
                     overlap. galaxy-gm motion track films/ram --thing <thing> says how far \
                     the thing itself was pushed.",
                ],
            },
            Recipe {
                title: "Compare a module change: film before and after, then compare",
                steps: &[
                    "galaxy film --modules game/modules --inputs keys.toml --from 0 --to 300 \
                     --probes-only --out films/before",
                    "Make the change to the module, then film the same inputs and ticks again",
                    "galaxy film --modules game/modules --inputs keys.toml --from 0 --to 300 \
                     --probes-only --out films/after",
                    "galaxy-gm motion compare films/before films/after",
                    "Read behaviour: the followed thing's path, top speed, arrival and end in \
                     each, side by side. For one thing in detail, run the same track on both:",
                    "galaxy-gm motion track films/after --thing ship --to <target>",
                    "Film without --probes-only (at one --size) to compare pixels too.",
                ],
            },
        ],
    },
    Topic {
        name: "modules",
        title: "Modules: checking them and what scripts can use",
        about: "A module is a folder with manifest.toml (id, versions, scripts, the events and \
                components it declares, what it writes, keys), start.toml (its entities), \
                scripts/*.luau, models/*.gltf and style.toml. IDs are `<module>.<name>`. \
                Places, beings, doors, spots and clocks are world components. Scripts use \
                ctx.move, ctx.walk and ctx.advance_clock; types lists their signatures, \
                fields components lists their stored fields. Walking runs in Rust. \
                Scripts run on events and once a second, never per frame; the engine's \
                services fly, move and collide things. An entry script may also return screen = \
                function(ctx, ui), returning ui.panel/text/button/list/row/column tables with \
                children. Screen ctx.get/query are read-only and tracked: Luau rebuilds only \
                when their data changes. Return nil to hide it. Buttons use on and payload; \
                clicks record that event with source ui. Dimensions are logical pixels, text \
                at least 16px. anchor = publisher#name follows an engine.anchor. check lays \
                out screens at 1280x720 and names problems by widget path. Bot [[click]] \
                entries (tick and either x/y or widget path/id) use galaxy play; then replay, audit or film the recording.",
        tasks: &[
            ("Check a module loads", "gm check"),
            ("What scripts can call", "gm types"),
            (
                "What a model is made of: parts, outline, morph, glow",
                "gm model",
            ),
        ],
        recipes: &[],
    },
    Topic {
        name: "rooms",
        title: "Rooms, floor spots, anchors and travel",
        about: "A place with engine.room and engine.model is a 3D glTF set. Its named camera node supplies the pose (glTF -Z forward, +Y up); engine.room.fov supplies vertical degrees. Each engine.spot has a matching spot.<name> node, imported into the world at start. Beings stand on these floor spots. Raised floors need matching engine.surface data. Room models use unit scale at the local origin. Moving the player starts one one-second cut; a click lands it. Leaving for an ancestor reverses its fades. engine.room.glide = seconds (0 to 10) replaces the cut into that room with a camera glide from the camera shown before, with no fade, and leaving it for a place that isn't a room glides back out to the follow camera; patching that room's engine.room.camera to another node while the player is there glides too, and play then records engine.camera_settled. A camera change or a trip that starts while a glide has not landed carries on from where that glide's camera is, with no jump. A click lands a glide on the next tick. Play records engine.place_arrived with entity, place and start_tick; scripts may subscribe to it. Arrival is a world input, not a zoom event. No Lua runs per frame. Anchors are place.<full-id>, spot.<name>, being.<full-id> and being.<full-id>.<node-name>, including face. check enters every room and rejects covered or offscreen beings. Audit and film reconstruct the cut from the saved trip and tick interpolation.",
        tasks: &[
            ("Room and travel component fields", "gm fields"),
            ("Validate every room", "gm check"),
        ],
        recipes: &[
            Recipe {
                title: "Add a room",
                steps: &[
                    "Model it: a glTF set at unit scale with the floor at the origin. Name one node camera (it looks down its -Z, +Y up) and add one empty node spot.<name> per floor spot, such as spot.entry, at floor height 0. Save it as models/cellar.gltf in the module, which makes the model `<module>.cellar`.",
                    "Declare it in start.toml. The examples use a module with id demo beside an existing place demo.station; write your manifest's id instead of demo (rooms in engine/tests/play/rooms/demo): [entities.cellar] with \"engine.place\" = { parent = 'demo.station', arrival = 'demo.cellar_entry', width = 10, depth = 8 }, \"engine.room\" = { title = 'Cellar' } and \"engine.model\" = { model = 'demo.cellar' }; then [entities.cellar_entry] with \"engine.spot\" = { place = 'demo.cellar', name = 'entry' }, one entity per spot node.",
                    "List engine.place, engine.room, engine.spot and engine.model in the manifest's writes (and engine.location, engine.position and engine.being for beings).",
                    "A being in the room: [entities.keeper] with \"engine.being\" = { player = false }, \"engine.location\" = { place = 'demo.cellar', spot = 'demo.cellar_entry' }, \"engine.position\" = {} and \"engine.model\" = { model = 'demo.being' }. Its spot sets where it stands.",
                    "A way in: a screen button with on = 'go' and payload = { place = 'demo.cellar' }, and an event handler go = function(event, ctx) ctx.move('player', event.payload.place) end, as engine/tests/play/rooms/demo/scripts/main.luau does. The player lands on the place's arrival spot. Beings whose day walks between rooms also want an engine.door { from, spot, to, arrival } each way.",
                    "galaxy-gm fields components --component room, then place, spot and door, lists every field.",
                    "galaxy-gm check <module folder>: it enters every room, the new one too, and flags beings covered or off screen from the room camera, and spot nodes that are missing.",
                    "galaxy play --modules <modules folder>: enter the room, press F8 for a note, quit, then galaxy-gm audit <session>/recording.json draws the moment again.",
                ],
            },
            Recipe {
                title: "Play both directions and reproduce an F8 note",
                steps: &[
                    "From the repository root, select the installed dev build with --build <name> immediately after galaxy or galaxy-gm.",
                    "galaxy-gm check engine/tests/play/rooms/demo",
                    "galaxy play --modules engine/tests/play/rooms --inputs engine/tests/play/rooms-inputs.toml --ticks 160",
                    "Use the session folder printed by play for the next commands. The bot enters and leaves both rooms, lands one cut by clicking, and takes notes in rooms and mid-cut.",
                    "galaxy sim replay <session>/recording.json",
                    "galaxy-gm audit <session>/recording.json",
                    "galaxy-gm film <session>/recording.json --from 0 --to 45 --size 1280x720 --out room-film",
                    "galaxy-gm guide motion",
                    "Inspect the notes, audit pictures and film frames. Compare the cut title and the beings' floor contact and clearance from the panel.",
                ],
            },
        ],
    },
    Topic {
        name: "talk",
        title: "Conversations on anchors and visible speaker focus",
        about: "An engine.talk entity names speaker and first (an engine.talk_line entity). Each line has text and optional next. A line can instead offer answers: engine.talk_choice { line, text, next, order } entities show as buttons in its bubble (lowest order first); a click records engine.talk_chosen {talk,line,choice} and continues to that choice's next, or ends the talk (seen) when empty. A stale answer records no event. A declined offer stays away by module logic, for example patching its talk enabled=false in the handler. The bubble's heading is title, else the speaker's life sign name; subtitle is a quieter line under it; Continue/Done is a small marker; [screen] bubble_* keys size them. An empty anchor uses being.<speaker>; otherwise use any published anchor, including publisher#name on an object without engine.being. The engine shows one bubble, advances on a click and starts again on a speaker click. story=true prioritizes an unseen moment; seen becomes true only when the last line is acknowledged. Leaving interrupts without losing the moment. enabled gates availability. Scripts use ordinary put/patch and the engine.talk capability to offer new moments. Bubbles use screen layout and neutral screen paint; width/x/y set logical-pixel placement, not colours. A being's focus defaults to the face mesh part. engine.focus { part = 'front' } names another mesh part; part='' explicitly opts out. Other anchors need no focus unless supplied. check composes every line with its place, room camera, screens, bubble and active walks at tick and half-tick poses. It rejects focus pixels covered by any model (including the speaker's own hat), widgets or bubble, and focus outside the viewport. Missing focus mesh parts fail too. This checks the authored state and active walks, not all future script branches; run check on authored variants and inspect recorded play. While playing, the same test runs on every frame a line shows: a speaker whose focus a script covered or moved off screen appears in the problems panel and in play's problems report. Inputs engine.talk_start {talk} and engine.talk_next {talk,line} record progress. Save, audit and film rebuild it without a private conversation timer.",
        tasks: &[
            ("Talk, line and focus fields", "gm fields"),
            ("Check focus in every place", "gm check"),
        ],
        recipes: &[
            Recipe {
                title: "Author, play, interrupt and reproduce a conversation",
                steps: &[
                    "Add engine.talk and engine.talk_line to manifest writes. In start.toml: [entities.hello] with \"engine.talk\" = { speaker = 'demo.visitor', first = 'demo.line', story = true }; [entities.line] with \"engine.talk_line\" = { text = 'Welcome.' }. The speaker needs a visible anchor; a being model needs a face mesh part, or an explicit engine.focus part.",
                    "galaxy-gm fields components --component talk",
                    "galaxy-gm fields components --component talk_line",
                    "galaxy-gm check engine/tests/play/talk/demo",
                    "galaxy-gm check engine/tests/play/talk-covered/demo",
                    "The second check deliberately fails: the third being's face is covered by its own geometry in Room B, although the player starts elsewhere.",
                    "galaxy play --modules engine/tests/play/talk --inputs engine/tests/play/talk-inputs.toml --ticks 380",
                    "Select the dev build with --build <name> immediately after each executable. In live play, enter Room A, click the first being, leave midway, return and finish. Click it again for repeat talk. Move the second speaker during its line. Room B has a cover/uncover test button: press it while the third being speaks and the problems panel flags the covered face, which check can't see. The hall's smaller figure has an anchor but no being component.",
                    "Bot [[click]] widget='speaker.<full-id>' clicks a visible model; widget='talk.<full-conversation-id>' clicks its bubble; widget='choice.<full-choice-id>' an answer. Anchor-only speakers use x/y. Use the session folder printed by play below.",
                    "galaxy sim replay <session>/recording.json",
                    "galaxy-gm audit <session>/recording.json",
                    "galaxy-gm film <session>/recording.json --from 225 --to 300 --size 1280x720 --out talk-film",
                    "galaxy-gm motion track talk-film --thing talk.second",
                    "galaxy-gm motion sheet talk-film --count 8",
                    "galaxy-gm guide motion",
                ],
            },
            Recipe {
                title: "Offer answers in a styled bubble",
                steps: &[
                    "Add engine.talk_choice to manifest writes. Give a line no next and two engine.talk_choice entities naming it; handle engine.talk_chosen in a script (event.payload.choice).",
                    "galaxy-gm fields components --component talk_choice",
                    "galaxy-gm check engine/tests/play/offer/demo",
                    "galaxy-gm check engine/tests/play/sign-panel/demo",
                    "The second check deliberately fails: its button opens a list over a name sign, which only check's presses reach.",
                    "galaxy play --modules engine/tests/play/offer --inputs engine/tests/play/offer-inputs.toml --ticks 120",
                    "The host's heading is its sign name with a quieter subtitle. Take it shows the next line; Not now ends the talk and the handler disables the offer, so the next click gets the repeat talk with Continue, then Done. Replay and audit the session as above.",
                ],
            },
        ],
    },
    Topic {
        name: "screens",
        title: "Screens, grids, scrolling and recorded gestures",
        about: "A manifest-listed Luau script returns screen = function(ctx, ui). Use \
                ui.panel/text/button/list/row/column with children = { ... }; return nil to \
                hide a screen. ctx.get/query track world reads, so Luau runs only when those \
                change. The engine supplies contrasting paper behind unbacked text. A button's \
                on names a declared event; its payload and source ui are recorded. \
                align = top-left, top, top-right, left, center, right, bottom-left, bottom \
                or bottom-right places a widget against the window's edges, with x/y as \
                margins, so it stays there at any window size and display scale. An \
                engine.anchor publisher follows a thing; anchor = publisher#name attaches a \
                widget, including a button. With yield = true it isn't drawn, rather than \
                clamped, while its anchor is off screen, it would leave the window or it \
                would overlap another painted widget. Bot clicks use widget = a full diagnostic path \
                or a unique id, or x/y logical pixels. Widget clicks resolve the current \
                centre even when its anchor moves; missing, ambiguous or covered targets fail. \
                A button with resting = \"<reason>\" can't act now: it stays in place, drawn \
                with [screen] resting and resting_dash, hovering shows the reason, a click \
                records nothing, and a bot click on it is listed in play's resting_clicks. \
                Its pad must be at least 1, room for its frame. \
                ui.grid uses columns; ui.scroll needs id and height and clips its children. \
                ui.delta uses a signed numeric value with gain/loss styling. ui.icon names an \
                icon in icons.toml [icons], mapping local names to glyph strings. check reports \
                missing icons and glyphs. ui.drag_source needs id, text and a string value; \
                ui.drop_target needs id, text, on and accepts = { value = reason }: an empty \
                reason allows it, a nonempty reason explains rejection, unlisted values are \
                rejected. notes = { value = text } says under Drop here what an accepted drop \
                changes. All verdicts and notes show at pick-up, reserved in layout so \
                nothing moves, and check reads them like other text; drop sends on with \
                payload plus value. \
                ui.scroll can have on/payload: a changed offset sends that event with offset. \
                Drag sources lift out of an empty socket and settle for 240ms on release. Targets \
                may contain children above the verdict. Store placement in world state and emit \
                the source at its new parent with the same unique id and value in that script. \
                Scroll offsets, thumb capture and token motion are preserved by notes and films. \
                Bot [[pointer]] takes tick, action = pickup/move/drop/scroll, widget = path/id, \
                and delta = logical pixels for scroll (positive down). Scrollbar parts use \
                widget = entries::thumb, entries::track-start or entries::track-end with \
                pickup/move/drop. Use separate ticks to \
                take a [[note]] mid-drag. Pointer gestures and clicks at one tick run gestures first.",
        tasks: &[("Screen API and types", "gm types")],
        recipes: &[
            Recipe {
                title: "Build a screen, attach a button, then check, click and replay it",
                steps: &[
                    "Create example/manifest.toml with:\nid = 'example'\nversion = '0.1.0'\nmin_engine_version = '0.4.0'\nscripts = ['screen.luau']\nevents = ['activate', 'clicked']\nwrites = ['engine.position', 'engine.motion', 'engine.camera', 'engine.anchor']",
                    "Create example/start.toml with:\n[entities.camera]\n'engine.position' = {}\n'engine.camera' = { view = 60 }\n[entities.marker]\n'engine.position' = { x = 12 }\n'engine.motion' = { vx = 2 }\n'engine.anchor' = { name = 'label' }",
                    "Create example/screen.luau with:\nreturn {\n  screen = function(_, ui)\n    return ui.panel { width = 320, children = {\n      ui.text { text = 'Screen example' },\n      ui.button { id = 'continue', anchor = 'marker#label',\n        width = 220, text = 'Continue', on = 'activate' },\n    } }\n  end,\n  on = { activate = function(_, ctx) ctx.record('clicked', {}) end },\n}",
                    "galaxy-gm check example",
                    "galaxy shot --modules example --ticks 1 --out screen.png",
                    "Inspect screen.png. check and shot report readability problems by widget path, including overlaps between screens. Fix any findings before continuing.",
                    "galaxy-gm fields inputs",
                    "Write clicks.toml with:\n[[click]]\ntick = 20\nwidget = 'continue'\n[[note]]\ntick = 25\ntext = 'The anchored button was clicked.'",
                    "galaxy play --modules example --inputs clicks.toml --ticks 30",
                    "Use the session folder printed by play as <session> in the next commands. The click records the button event, not its lookup name or coordinates.",
                    "galaxy sim replay <session>/recording.json",
                    "galaxy-gm recording <session>/recording.json inputs",
                    "galaxy-gm audit <session>/recording.json",
                    "Read the replay hash and audit differences; inspect the note and audit pictures. For a named dev build, add --build <name> immediately after galaxy or galaxy-gm in every command.",
                ],
            },
            Recipe {
                title: "Add scrolling and drag targets, then audit a mid-drag note",
                steps: &[
                    "In the example above, declare events place and scrolled in manifest.toml. Their handlers receive event.payload.value (a dropped string) or event.payload.offset (scroll pixels); record a clicked fact to observe them.",
                    "Add icons.toml with:\n[icons]\ntoken = 'T'",
                    "Inside the panel children, add ui.icon { icon = 'token' }, ui.delta { value = -12 }, and ui.grid { columns = 2, children = { ui.drag_source { id = 'token', text = 'Token', value = 'small' }, ui.drop_target { id = 'slot', text = 'Slot', on = 'place', accepts = { small = '', large = 'Too wide' }, notes = { small = 'Fills the slot' } } } }.",
                    "Add ui.scroll { id = 'entries', height = 160, on = 'scrolled', children = { ui.text { text = 'One', height = 80 }, ui.text { text = 'Two', height = 80 }, ui.text { text = 'Three', height = 80 } } }. Keep the panel within the viewport. All entries are checked even when clipped by scrolling.",
                    "Write gestures.toml with:\n[[pointer]]\ntick = 10\naction = 'scroll'\nwidget = 'entries'\ndelta = 80\n[[note]]\ntick = 18\ntext = 'Scrolled'\n[[pointer]]\ntick = 30\naction = 'pickup'\nwidget = 'token'\n[[note]]\ntick = 40\ntext = 'Mid-drag'\n[[pointer]]\ntick = 55\naction = 'drop'\nwidget = 'slot'",
                    "galaxy-gm check example",
                    "galaxy play --modules example --inputs gestures.toml --ticks 70",
                    "galaxy sim replay <session>/recording.json",
                    "galaxy-gm audit <session>/recording.json",
                    "galaxy-gm film <session>/recording.json --note 2 --before 0 --after 1",
                    "Inspect the scrolled and mid-drag note, audit and film images. In live play, wheel over the list, hold the left button on the source, and release over a target. F8 freezes the displayed frame, including an active drag.",
                ],
            },
        ],
    },
    Topic {
        name: "models",
        title: "Models from recipes: stations, hulls, parts and rocks",
        about: "A module's models can be made from recipes instead of drawn by hand: \
                recipes/<name>.toml makes models/<name>.gltf (galaxy-gm model make). A recipe \
                is [[part]] tables, each a shape with its keys (galaxy-gm fields recipe lists \
                every shape, key, range and default) or use = \"<recipe>\" for another recipe \
                of the module as a preset (a fitted part, a variant). Place a part with at = \
                [x, y, z] (x the model's front, y its left, z up) or angle and distance round \
                the centre, turned outwards; turn, pitch, roll and scale it; copies = n repeats \
                it round the centre and mirror = true adds the left-right copy. Paint is by \
                palette slot: recipes/palette.toml holds slot = \"#rrggbb\", a recipe's \
                [palette] adds or replaces slots, and a used recipe is painted with its \
                user's slots first (or a part's palette = { ... }). Shapes paint with paint \
                (slot main), trim, accent, lights (self-lit) and glass. Each [[part]] is one \
                glTF node, named by name: the part galaxy-gm model and the ink see. \
                [[marker]] adds empty named nodes: a room's camera and spot.<name>, which \
                look along turn and pitch. A rock with mined above 0 has a morph target: \
                engine.model morph runs it from whole to mined. spin = degrees a second \
                turns a part about its own up (copies together about the centre) and pulse \
                = seconds pulses its glow down to pulse_low: drawn only, by game time. The seed varies lumps and \
                chips; the same recipe gives the same bytes on every OS. Commit the recipes \
                and the models, and remake after changing a recipe.",
        tasks: &[
            ("Make models from a module's recipes", "gm model make"),
            ("Which shapes and keys a recipe takes", "gm fields"),
            ("What a made model is: parts, outline, morph", "gm model"),
        ],
        recipes: &[Recipe {
            title: "Make a rock and a small station and see them in the game",
            steps: &[
                "Write recipes/palette.toml with main, trim, accent, lights and glass slots, and recipes/rock.toml with one [[part]] of shape = \"rock\", radius = 2.0, mined = 0.5.",
                "Write recipes/station.toml: a tiers core, a spoked_ring round it and a truss with copies = 3, angle and distance.",
                "galaxy-gm fields recipe",
                "galaxy-gm model make <module>",
                "galaxy-gm model <module> station",
                "Give a thing in start.toml engine.model = { model = \"<id>.station\" } (<id> is the manifest's id), and another the rock with morph = 0.5 to show it part mined; then galaxy-gm check <module>.",
                "galaxy shot --modules <module> --out station.png --ticks 30",
            ],
        }],
    },
    Topic {
        name: "style",
        title: "How the world is drawn, and the keys files take",
        about: "style.toml sets the scene-wide ink, light, sky, course and screen (from one module \
                only) and named looks a model can wear (`look = \"<module>.<name>\"`). Every \
                key is optional. [sky] star_layers = [0.25, 0.6, 1.2] lays stars in layers \
                scrolling at their own rates. [screen] owns paper, panel, button, text, gain/loss, bubble/sign, held/socket, accept/refuse, resting (a resting button) and scroll colours; radius, frame_padding, target_min, handle_width/handle_min/handle_radius, border_px, socket_dash, resting_dash and hint_border_px; and talk bubble text sizes bubble_title (and bubble_title_weight), bubble_subtitle, bubble_text and bubble_marker. Colours must be opaque, all text/surface pairs need 4.5:1 contrast and handles on track 3:1. galaxy-gm fields style lists defaults and ranges. Last module ID supplying [screen] wins the whole table; check reports conflicts. Without a table, screens use the neutral default. Explicit widget pad overrides frame_padding; target_min is a checked floor, not automatic resizing. Style-only hot loads have no sim event. Effects, drawn only: a look's halo glows round its self-lit paint; [beams.<name>] (color, width, glow, flicker, impact = [recipes]) styles engine.beam { target, look, x, y, height }, a beam to target's surface; [particles.<name>] (shape dot/spark/puff/chip, blend add/paint, count, rate, life, speed, spread, size, drag) is sprayed by engine.emitter { recipe, x, y, angle, rate, by = thrust | speed } and engine.burst { recipe, tick = ctx.tick }; engine.camera speed_zoom, lead and shake with shake_tick = ctx.tick add camera feel, which galaxy play --reduced-motion turns off. A scene draws at most 4096 particles. Check a change with galaxy shot (a still) or a film and a \
                motion tool (for anything that turns or moves).",
        tasks: &[(
            "Which keys and fields exist; which things wear a look",
            "gm fields",
        )],
        recipes: &[],
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
        recipes: &[],
    },
];

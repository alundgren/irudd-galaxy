//! `galaxy-gm music <module> --name <sound> --prompt <text>`: generates a piece of music
//! through OpenRouter at module-build time, writes it into the module as
//! `music/<name>.<ext>` and declares it in the module's `audio.toml` (a sound, and a
//! music-bus cue with `--event`), so the sound system plays it like any authored file.
//!
//! Everything that can be judged without the call is judged first (the module, the name,
//! the event, whether it would overwrite something), then the key, then one call. The
//! game never calls a model; only this command does, when someone runs it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use galaxy_api::Registry;
use galaxy_module::cli::Output;
use galaxy_module::failure::code;
use galaxy_module::module::Module;
use galaxy_module::{Failure, discover, ids};
use galaxy_motion::Opts;
use serde_json::{Value, json};
use toml_edit::{DocumentMut, Item, Table, value};

use crate::openrouter::{self, ApiKey, CallError, Transport};

/// The options `music` takes.
pub const OPTIONS: &[&str] = &[
    "name", "prompt", "event", "volume", "fade", "model", "seed", "replace",
];

/// About 30 seconds of music. `google/lyria-3-pro-preview` makes a full piece.
pub const DEFAULT_MODEL: &str = "google/lyria-3-clip-preview";
/// Added to every prompt: background music for a game has no singing.
pub const INSTRUMENTAL: &str = "Instrumental only: no vocals, no singing, no lyrics.";
/// Longest prompt sent.
const MAX_PROMPT_CHARS: usize = 1000;
/// A full piece can take minutes to stream.
pub const TIMEOUT: Duration = Duration::from_secs(600);

/// What the command line asked for.
struct Ask {
    name: String,
    prompt: String,
    event: Option<String>,
    volume: Option<f64>,
    fade: f64,
    model: String,
    seed: Option<u64>,
    replace: bool,
}

fn ask(opts: &Opts) -> Result<Ask, String> {
    opts.only("music", OPTIONS)?;
    let name = opts
        .text("name")
        .ok_or("music needs --name <sound>, the sound's local name")?;
    if !ids::valid_field(name) {
        return Err(ids::name_rule(name));
    }
    let prompt: String = opts
        .text("prompt")
        .filter(|p| *p != "true")
        .ok_or("music needs --prompt <text>, what the music should sound like")?
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let prompt = prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(format!(
            "--prompt must be 1 to {MAX_PROMPT_CHARS} characters"
        ));
    }
    let number = |n: &str, lo: f32, hi: f32| -> Result<Option<f64>, String> {
        match opts.number(n)? {
            Some(v) if !(lo..=hi).contains(&v) => Err(format!("--{n} must be from {lo} to {hi}")),
            v => Ok(v.map(|v| (f64::from(v) * 1000.0).round() / 1000.0)),
        }
    };
    let seed = match opts.text("seed") {
        None => None,
        Some(s) => Some(
            s.parse::<u64>()
                .map_err(|_| format!("--seed must be a whole number, not `{s}`"))?,
        ),
    };
    let model = opts.text("model").unwrap_or(DEFAULT_MODEL);
    if model == "true" || model.is_empty() || model.chars().any(char::is_whitespace) {
        return Err("--model takes an OpenRouter model ID such as vendor/model".into());
    }
    let event = match opts.text("event") {
        Some("true") => return Err("--event takes an event kind".into()),
        e => e.map(str::to_owned),
    };
    if opts.text("replace").is_some_and(|v| v != "true") {
        return Err("--replace takes no value".into());
    }
    Ok(Ask {
        name: name.to_owned(),
        prompt: format!("{prompt} {INSTRUMENTAL}"),
        event,
        volume: number("volume", 0.0, 1.0)?,
        fade: number("fade", 0.0, 60.0)?.unwrap_or(1.0),
        model: model.to_owned(),
        seed,
        replace: opts.flag("replace"),
    })
}

/// The module at `dir` and the modules beside it, which may declare its events.
fn with_siblings(dir: &Path, module: &Module) -> Vec<Module> {
    let mut all = Vec::new();
    if let Some(parent) = dir.parent() {
        for other in discover::modules_in(parent) {
            if other != dir
                && let Ok(m) = Module::read(&other)
                && m.id() != module.id()
            {
                all.push(m);
            }
        }
    }
    all.push(module.clone());
    all
}

/// Runs the command: `key` is `OPENROUTER_API_KEY`, `transport` sends the one call.
pub fn run(
    dir: &Path,
    opts: &Opts,
    key: Option<ApiKey>,
    transport: &mut dyn Transport,
) -> Result<Output, Failure> {
    let mut ask = ask(opts).map_err(Failure::usage)?;
    let module = Module::read(dir)?;
    let bad = |m: String| Failure::bad_input(m).with("path", dir.display().to_string());

    // Judge everything that needs no call.
    let toml_path = dir.join("audio.toml");
    let mut doc: DocumentMut = match std::fs::read_to_string(&toml_path) {
        Ok(text) => text
            .parse()
            .map_err(|e| bad(format!("audio.toml is not valid TOML: {e}")))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
        Err(e) => return Err(bad(format!("can't read audio.toml: {e}"))),
    };
    let existing = doc
        .get("sounds")
        .and_then(Item::as_table_like)
        .and_then(|t| t.get(&ask.name))
        .map(|s| {
            s.get("file")
                .and_then(Item::as_str)
                .unwrap_or("")
                .to_owned()
        });
    let taken: Vec<String> = ["mp3", "wav", "ogg"]
        .iter()
        .map(|ext| format!("music/{}.{ext}", ask.name))
        .filter(|f| dir.join(f).exists())
        .collect();
    if !ask.replace && (existing.is_some() || !taken.is_empty()) {
        let what = match &existing {
            Some(file) => format!("audio.toml already has the sound `{}` ({file})", ask.name),
            None => format!("{} already exists", taken.join(", ")),
        };
        return Err(bad(format!(
            "{what}; pass --replace to generate it again, or choose another --name"
        )));
    }
    let event = match &ask.event {
        None => None,
        Some(e) => {
            let kind = if e.contains('.') {
                e.clone()
            } else {
                module.full(e)
            };
            let all = with_siblings(dir, &module);
            let registry = Registry::new(&all.iter().collect::<Vec<_>>()).map_err(bad)?;
            if !registry.events.contains(&kind) {
                return Err(bad(format!(
                    "event `{kind}` is not declared: add `{e}` to the manifest's events, or \
                     use an engine event such as engine.start"
                )));
            }
            Some(e.clone())
        }
    };

    let Some(key) = key else {
        return Err(Failure::new(
            code::CANNOT_START,
            "no_key",
            format!(
                "{} is not set. galaxy-gm music generates through OpenRouter and needs that \
                 key in the environment; nothing was generated or written",
                ApiKey::ENV
            ),
        )
        .with("env", ApiKey::ENV));
    };

    // A key pasted into a prompt goes nowhere: not to the model, the file or the output.
    ask.prompt = key.redact(&ask.prompt);
    let body = openrouter::music_body(&ask.model, &ask.prompt, ask.seed);
    let audio = openrouter::generate(transport, &key, &body).map_err(|e| {
        let failure = |status: Value, message: String| {
            Failure::new(code::RUNTIME, "openrouter", message)
                .with("status", status)
                .with("model", ask.model.clone())
                .with("written", false)
        };
        match e {
            CallError::Network(m) => {
                failure(Value::Null, format!("no answer from OpenRouter: {m}"))
            }
            CallError::Refused { status, message } => failure(
                json!(status),
                format!("OpenRouter refused the call (HTTP {status}): {message}"),
            ),
            CallError::Malformed {
                message,
                answer,
                generation_id,
                cost_usd,
            } => failure(
                Value::Null,
                format!(
                    "OpenRouter's answer has no usable audio: {message}. Nothing was \
                     written; running again makes a new call"
                ),
            )
            .with("answer", answer.unwrap_or(Value::Null))
            .with("generation_id", generation_id)
            .with("cost_usd", cost_usd),
        }
    })?;

    // Write the file next to where it ends up, check it decodes, then move it in place.
    let file = format!("music/{}.{}", ask.name, audio.format.ext());
    let target = dir.join(&file);
    let partial = dir.join(format!("music/{}-partial.{}", ask.name, audio.format.ext()));
    let io = |what: &str, e: std::io::Error| {
        Failure::runtime(format!("{what}: {e}")).with("path", dir.display().to_string())
    };
    std::fs::create_dir_all(dir.join("music")).map_err(|e| io("can't create music/", e))?;
    std::fs::write(&partial, &audio.bytes).map_err(|e| io("can't write the music", e))?;
    let clip = match galaxy_audio::Clip::read(&partial) {
        Ok(clip) => clip,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return Err(Failure::new(
                code::RUNTIME,
                "openrouter",
                format!("the generated audio does not decode: {e}"),
            )
            .with("status", Value::Null)
            .with("model", ask.model.clone())
            .with("written", false));
        }
    };
    std::fs::rename(&partial, &target).map_err(|e| io("can't move the music in place", e))?;

    let replaced = declare(
        &mut doc,
        &ask,
        &file,
        event.as_deref(),
        audio.model.as_deref(),
    );
    write_atomic(&toml_path, doc.to_string().trim_start())
        .map_err(|e| io("can't write audio.toml", e))?;

    // What the sound system now makes of this module.
    let reread = Module::read(dir)?;
    let all = with_siblings(dir, &reread);
    let findings: Vec<Value> = galaxy_audio::Bank::load(&all)
        .problems
        .into_iter()
        .filter(|p| p.starts_with(&format!("{}:", reread.id())))
        .map(|p| json!({ "kind": "sound", "message": p }))
        .collect();
    let seconds = clip.frames() as f64 / f64::from(clip.sample_rate());
    let next = match &event {
        Some(_) => format!(
            "galaxy-gm check {0}; then galaxy play --modules {1} and listen.",
            dir.display(),
            dir.parent().unwrap_or(dir).display()
        ),
        None => format!(
            "Nothing cues it yet: add an [[events]] entry with sound = \"{}\" and bus = \"music\" \
             to audio.toml (or run again with --event and --replace), then galaxy-gm check.",
            ask.name
        ),
    };
    Ok(Output::Json(json!({
        "ok": findings.is_empty(),
        "module": reread.id(),
        "sound": ask.name,
        "file": file,
        "format": audio.format.ext(),
        "bytes": audio.bytes.len(),
        "seconds": (seconds * 100.0).round() / 100.0,
        "sample_rate": clip.sample_rate(),
        "event": event,
        "replaced": replaced,
        "model": ask.model,
        "answered_by": audio.model,
        "prompt": ask.prompt,
        "seed": ask.seed,
        "text": audio.text,
        "cost_usd": audio.cost_usd,
        "generation_id": audio.generation_id,
        "findings": findings,
        "next": next,
    })))
}

/// Declares the sound (and its cue) in `doc`. Returns the file the sound named before,
/// if it existed.
fn declare(
    doc: &mut DocumentMut,
    ask: &Ask,
    file: &str,
    event: Option<&str>,
    answered_by: Option<&str>,
) -> Option<String> {
    let note = format!(
        "\n# galaxy-gm music ({}): {}\n",
        answered_by.unwrap_or(&ask.model),
        ask.prompt.trim_end_matches(INSTRUMENTAL).trim()
    );
    if doc.get("sounds").and_then(Item::as_table_like).is_none() {
        let mut t = Table::new();
        t.set_implicit(true);
        doc.insert("sounds", Item::Table(t));
    }
    let sounds = doc["sounds"]
        .as_table_like_mut()
        .expect("sounds is a table");
    let before = sounds.get(&ask.name).map(|s| {
        (
            s.get("file")
                .and_then(Item::as_str)
                .unwrap_or("")
                .to_owned(),
            s.get("volume").and_then(Item::as_float),
        )
    });
    let volume = ask
        .volume
        .or_else(|| before.as_ref().and_then(|b| b.1))
        .unwrap_or(0.5);
    match sounds.get_mut(&ask.name) {
        Some(Item::Table(t)) => {
            t["file"] = value(file);
            t["volume"] = value(volume);
            t.decor_mut().set_prefix(note);
        }
        Some(Item::Value(toml_edit::Value::InlineTable(t))) => {
            t.insert("file", file.into());
            t.insert("volume", volume.into());
        }
        _ => {
            let mut t = Table::new();
            t["file"] = value(file);
            t["volume"] = value(volume);
            t.decor_mut().set_prefix(note);
            sounds.insert(&ask.name, Item::Table(t));
        }
    }
    if let Some(event) = event {
        let has = |e: &dyn toml_edit::TableLike| {
            e.get("event").and_then(Item::as_str) == Some(event)
                && e.get("sound").and_then(Item::as_str) == Some(ask.name.as_str())
                && e.get("bus").and_then(Item::as_str) == Some("music")
        };
        let mut cue = Table::new();
        cue["event"] = value(event);
        cue["sound"] = value(ask.name.as_str());
        cue["bus"] = value("music");
        cue["fade"] = value(ask.fade);
        match doc.get_mut("events") {
            Some(Item::ArrayOfTables(list)) => {
                if !list.iter().any(|e| has(e)) {
                    list.push(cue);
                }
            }
            Some(Item::Value(toml_edit::Value::Array(list))) => {
                if !list
                    .iter()
                    .any(|e| e.as_inline_table().is_some_and(|e| has(e)))
                {
                    list.push(cue.into_inline_table());
                }
            }
            _ => {
                let mut list = toml_edit::ArrayOfTables::new();
                list.push(cue);
                doc.insert("events", Item::ArrayOfTables(list));
            }
        }
    }
    before.map(|b| b.0)
}

/// Writes beside the target, then moves it over, so a failed write leaves the old file.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp: PathBuf = path.with_extension("toml.partial");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openrouter::tests::{Canned, canned, stream};

    const KEY: &str = "sk-or-v1-feedfacefeedfacefeedfacefeedface";

    fn tone() -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/play/audio/demo/sounds/high.mp3"),
        )
        .unwrap()
    }

    fn module(audio: Option<&str>) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("calm");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            "id = 'calm'\nversion = '0.1.0'\nmin_engine_version = '0.4.0'\nevents = ['arrive']\n",
        )
        .unwrap();
        if let Some(audio) = audio {
            std::fs::write(dir.join("audio.toml"), audio).unwrap();
        }
        tmp
    }

    fn opts(args: &str) -> Opts {
        let mut words: Vec<String> = Vec::new();
        // `--prompt` takes the rest up to the next option, so tests can write prose.
        for part in args.split(" --").filter(|p| !p.is_empty()) {
            let part = part.trim_start_matches("--");
            match part.split_once(' ') {
                Some((k, v)) => {
                    words.push(format!("--{k}"));
                    words.push(v.to_owned());
                }
                None => words.push(format!("--{part}")),
            }
        }
        Opts::parse(&words).unwrap()
    }

    fn fail(r: Result<Output, Failure>) -> Failure {
        match r {
            Err(f) => f,
            Ok(_) => panic!("expected a failure"),
        }
    }

    fn run_with(dir: &Path, args: &str, t: &mut Canned) -> Result<Value, Failure> {
        run(dir, &opts(args), Some(ApiKey::new(KEY)), t).map(|o| match o {
            Output::Json(v) => v,
            Output::Text(t) => panic!("{t}"),
        })
    }

    #[test]
    fn generated_music_is_written_declared_cued_and_plays() {
        let tmp = module(Some(
            "# The author's own note stays.\n[sounds.ping]\nfile = 'ping.wav'\n",
        ));
        let dir = tmp.path().join("calm");
        std::fs::write(dir.join("ping.wav"), b"").unwrap();
        let mut t = canned(200, stream(&tone(), 2));
        let out = run_with(
            &dir,
            "--name theme --prompt slow warm pads, gentle --event engine.start --volume 0.4",
            &mut t,
        )
        .unwrap();
        assert_eq!(out["file"], "music/theme.mp3");
        assert_eq!(out["format"], "mp3");
        assert_eq!(out["sample_rate"], 16000);
        assert_eq!(out["cost_usd"], 0.04);
        assert_eq!(out["event"], "engine.start");
        assert_eq!(std::fs::read(dir.join("music/theme.mp3")).unwrap(), tone());
        assert!(!dir.join("music/theme-partial.mp3").exists());
        let sent: Value = serde_json::from_str(&t.sent[0].2).unwrap();
        let prompt = sent["messages"][0]["content"].as_str().unwrap();
        assert!(prompt.starts_with("slow warm pads, gentle") && prompt.ends_with(INSTRUMENTAL));
        assert_eq!(sent["model"], DEFAULT_MODEL);
        let text = std::fs::read_to_string(dir.join("audio.toml")).unwrap();
        assert!(text.contains("# The author's own note stays."), "{text}");
        assert!(
            text.contains("# galaxy-gm music (vendor/music-clip): slow warm pads"),
            "{text}"
        );
        let v: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(
            v["sounds"]["theme"]["file"].as_str(),
            Some("music/theme.mp3")
        );
        assert_eq!(v["sounds"]["theme"]["volume"].as_float(), Some(0.4));
        let cue = &v["events"][0];
        assert_eq!(cue["event"].as_str(), Some("engine.start"));
        assert_eq!(cue["bus"].as_str(), Some("music"));
        // The sound system loads the new sound and cue; the empty ping.wav is the only
        // finding, and it was there before.
        let findings = out["findings"].as_array().unwrap();
        assert_eq!(findings.len(), 1, "{out}");
        assert!(findings[0]["message"].as_str().unwrap().contains("ping"));
        assert!(!out.to_string().contains("feedface"));
        assert!(!text.contains("feedface"));
    }

    #[test]
    fn nothing_is_overwritten_without_replace_and_replace_keeps_one_cue() {
        let tmp = module(None);
        let dir = tmp.path().join("calm");
        let args = "--name theme --prompt calm --event arrive";
        run_with(&dir, args, &mut canned(200, stream(&tone(), 1))).unwrap();
        let mut t = canned(200, stream(&tone(), 1));
        let e = run_with(&dir, args, &mut t).unwrap_err();
        assert_eq!((e.code, e.kind), (4, "bad_input"), "{}", e.message);
        assert!(e.message.contains("--replace"), "{}", e.message);
        assert!(t.sent.is_empty(), "no call is made for a refused write");
        let out = run_with(&dir, &format!("{args} --replace"), &mut t).unwrap();
        assert_eq!(out["replaced"], "music/theme.mp3");
        assert_eq!(out["ok"], true, "{out}");
        let v: toml::Value =
            toml::from_str(&std::fs::read_to_string(dir.join("audio.toml")).unwrap()).unwrap();
        assert_eq!(v["events"].as_array().unwrap().len(), 1);
        // The volume a sound had is kept when --volume is not given.
        assert_eq!(v["sounds"]["theme"]["volume"].as_float(), Some(0.5));
    }

    #[test]
    fn bad_asks_fail_before_the_key_and_the_call() {
        let tmp = module(None);
        let dir = tmp.path().join("calm");
        for (args, kind) in [
            ("--prompt calm", "usage"),
            ("--name Theme --prompt calm", "usage"),
            ("--name theme", "usage"),
            ("--name theme --prompt calm --volume 2", "usage"),
            ("--name theme --prompt calm --colour red", "usage"),
            ("--name theme --prompt calm --event nowhere", "bad_input"),
        ] {
            let mut t = canned(200, "unused");
            let e = fail(run(&dir, &opts(args), None, &mut t));
            assert_eq!(e.kind, kind, "{args}: {}", e.message);
            assert!(t.sent.is_empty());
        }
        let e = fail(run(
            &tmp.path().join("missing"),
            &opts("--name a --prompt b"),
            None,
            &mut canned(200, ""),
        ));
        assert_eq!(e.kind, "bad_input");
    }

    #[test]
    fn without_a_key_it_says_so_and_writes_nothing() {
        let tmp = module(None);
        let dir = tmp.path().join("calm");
        let mut t = canned(200, stream(&tone(), 1));
        let e = fail(run(&dir, &opts("--name theme --prompt calm"), None, &mut t));
        assert_eq!((e.code, e.kind), (2, "no_key"));
        assert!(
            e.message.contains("OPENROUTER_API_KEY is not set"),
            "{}",
            e.message
        );
        assert!(t.sent.is_empty());
        assert!(!dir.join("music").exists() && !dir.join("audio.toml").exists());
    }

    #[test]
    fn a_refused_or_unplayable_answer_writes_nothing() {
        let tmp = module(None);
        let dir = tmp.path().join("calm");
        let refusal =
            format!(r#"{{"error":{{"code":402,"message":"Insufficient credits for {KEY}"}}}}"#);
        let e = run_with(
            &dir,
            "--name theme --prompt calm",
            &mut canned(402, refusal),
        )
        .unwrap_err();
        assert_eq!((e.code, e.kind), (1, "openrouter"));
        assert_eq!(e.fields["status"], 402);
        assert!(!e.message.contains("feedface") && !e.to_json().to_string().contains("feedface"));
        let broken = stream(b"ID3\x03\x00\x00\x00\x00\x00\x00not really audio", 1);
        let e = run_with(&dir, "--name theme --prompt calm", &mut canned(200, broken)).unwrap_err();
        assert_eq!(e.kind, "openrouter");
        assert!(e.message.contains("does not decode"), "{}", e.message);
        let left: Vec<_> = std::fs::read_dir(dir.join("music")).unwrap().collect();
        assert!(left.is_empty(), "no partial file is left");
        assert!(!dir.join("audio.toml").exists());
    }
}

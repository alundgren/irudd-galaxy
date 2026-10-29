//! Live calls to OpenRouter. Ignored by default; they run only with
//! `OPENROUTER_API_KEY` set:
//!
//! ```sh
//! cargo test -p galaxy-mind --test live -- --ignored --nocapture
//! ```

use std::path::Path;

use galaxy_mind::config::MindConfig;
use galaxy_mind::{ApiKey, OpenRouterMind, probe};

fn config() -> MindConfig {
    MindConfig::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data"))
        .expect("core mind.toml loads")
}

fn live_mind(config: &MindConfig) -> Option<OpenRouterMind> {
    if ApiKey::from_env().is_err() {
        eprintln!("skipped: {} is not set", ApiKey::ENV);
        return None;
    }
    Some(OpenRouterMind::from_env(config.models.clone(), &config.limits).expect("key is set"))
}

#[test]
#[ignore = "calls OpenRouter; needs OPENROUTER_API_KEY"]
fn live_jev_picks_an_option() {
    let config = config();
    let Some(mut mind) = live_mind(&config) else {
        return;
    };
    let r = probe::decision(&mut mind);
    eprintln!("{}", serde_json::to_string_pretty(&r).unwrap());
    assert!(r.ok, "{:?}", r.error);
    let answer = r.answer.unwrap();
    assert!(
        answer["option"].is_string(),
        "the choice is on the menu: {answer}"
    );
}

#[test]
#[ignore = "calls OpenRouter; needs OPENROUTER_API_KEY"]
fn live_gpt_oss_says_a_grounded_line() {
    let config = config();
    let Some(mut mind) = live_mind(&config) else {
        return;
    };
    // A few samples: how often a real line passes the grounding checks.
    const SAMPLES: usize = 5;
    let (mut answered, mut valid) = (0, 0);
    for _ in 0..SAMPLES {
        let r = probe::line(&mut mind, &config);
        eprintln!("{}", serde_json::to_string_pretty(&r).unwrap());
        // A call can fail (the model sometimes returns no content); the host then
        // says the authored line. Count it rather than stop.
        let Some(answer) = r.answer else {
            continue;
        };
        answered += 1;
        assert_eq!(answer["cites"][0], "fact.40", "{answer}");
        valid += usize::from(answer["valid"] == true);
    }
    eprintln!("{answered} of {SAMPLES} calls answered; {valid} lines passed validation");
    assert!(valid > 0, "no real line passed validation");
}

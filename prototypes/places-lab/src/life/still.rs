//! Still: people stay where the content put them, idling; a story moment moves them at
//! once, unseen.

use super::Life;

pub struct Still;

impl Life for Still {
    fn name(&self) -> &'static str {
        "still"
    }
}

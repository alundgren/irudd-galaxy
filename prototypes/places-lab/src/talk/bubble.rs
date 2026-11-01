//! A bubble on the person: the line hangs over the speaker's head, its tail pointing at
//! them.

use super::{Line, Talk};
use crate::widgets::W;

pub struct Bubble;

impl Talk for Bubble {
    fn name(&self) -> &'static str {
        "bubble"
    }

    fn widgets(&self, line: &Line) -> Vec<W> {
        vec![
            W::new("bubble")
                .set("id", "bubble")
                .set("anchor", format!("person.{}", line.who))
                .set("speaker", line.name.as_str())
                .set("place_name", line.role.as_str())
                .set("text", line.text.as_str())
                .set("more", line.more)
                .set("on", line.on.clone())
                .set("args", line.args.clone()),
        ]
    }
}

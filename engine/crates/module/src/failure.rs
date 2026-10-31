//! Exit codes and the one JSON object a failed command prints.
//!
//! The table and every `kind` are listed in `engine/docs/exit-codes.md`; keep them in step.

use std::process::ExitCode;

use serde_json::{Map, Value};

/// Exit codes shared by `galaxy`, `galaxy-gm` and their launchers.
pub mod code {
    /// The command did what it was asked.
    pub const OK: u8 = 0;
    /// The command started but failed: I/O, a crash it caught, or a command that is not
    /// built yet.
    pub const RUNTIME: u8 = 1;
    /// The command could not start: bad usage, an engine too old for the modules, or no
    /// installed build to run.
    pub const CANNOT_START: u8 = 2;
    /// The input is wrong: a missing or invalid module or manifest.
    pub const BAD_INPUT: u8 = 4;
    /// The input was read and judged, and rejected (a check with findings).
    pub const REJECTED: u8 = 5;
}

/// A failed command: its exit code, a `kind` that tells cases apart, a message for
/// people and any fields that help an agent act on it.
#[derive(Debug, Clone)]
pub struct Failure {
    pub code: u8,
    pub kind: &'static str,
    pub message: String,
    pub fields: Map<String, Value>,
}

impl Failure {
    pub fn new(code: u8, kind: &'static str, message: impl Into<String>) -> Self {
        Failure {
            code,
            kind,
            message: message.into(),
            fields: Map::new(),
        }
    }

    /// Exit 2, kind `usage`.
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(code::CANNOT_START, "usage", message)
    }

    /// Exit 4, kind `bad_input`.
    pub fn bad_input(message: impl Into<String>) -> Self {
        Self::new(code::BAD_INPUT, "bad_input", message)
    }

    /// Exit 1, kind `runtime`.
    pub fn runtime(message: impl Into<String>) -> Self {
        Self::new(code::RUNTIME, "runtime", message)
    }

    /// Adds a field to the JSON object.
    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.fields.insert(key.to_owned(), value.into());
        self
    }

    /// `{"kind": ..., "error": ..., <fields>}`.
    pub fn to_json(&self) -> Value {
        let mut object = self.fields.clone();
        object.insert("kind".into(), self.kind.into());
        object.insert("error".into(), self.message.clone().into());
        Value::Object(object)
    }

    /// Prints the JSON object on stdout and returns the exit code.
    pub fn report(&self) -> ExitCode {
        crate::cli::print_json(&self.to_json());
        ExitCode::from(self.code)
    }
}

#[cfg(test)]
mod tests {
    use super::Failure;

    #[test]
    fn the_object_carries_kind_error_and_fields() {
        let json = Failure::usage("no command")
            .with("usage", "galaxy <command>")
            .to_json();
        assert_eq!(json["kind"], "usage");
        assert_eq!(json["error"], "no command");
        assert_eq!(json["usage"], "galaxy <command>");
    }
}

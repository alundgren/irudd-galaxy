# Deliberately bad packs

Each folder is a pack with exactly one thing wrong. `galaxy pack check` must reject it
with a precise report (file, line, reason); `crates/script/tests/bad_packs.rs` keeps it so.

| Pack | What's wrong | Rejected at | Code |
|---|---|---|---|
| `undeclared_hook` | Writes a stage and a talk line with only read hooks declared | hooks (and types) | `undeclared_hook` |
| `infinite_loop` | A handler that never returns | smoke | `over_budget` |
| `os_time` | Reads the wall clock | invariants | `clock_api` |
| `uncited_talk` | A talk line without `cites` | invariants (and types) | `uncited_line` |
| `unpaired_shock` | A shock whose opportunity isn't declared | invariants | `unpaired_shock` |
| `timer_trigger` | `G.after { seconds = 30 }` | hooks | `bad_trigger` |
| `nocheck` | `--!nocheck` turns the strict typecheck off | types | `typecheck_off` |
| `bare_iteration` | A bare `for k in t` loop, whose order isn't stable | invariants | `unordered_iteration` |

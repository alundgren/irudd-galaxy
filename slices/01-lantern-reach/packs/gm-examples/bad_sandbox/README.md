# Packs the sandbox must reject

Each pack here passes `galaxy pack check` (the fast checks) and fails only when
`galaxy gm propose` runs it in the sandbox: a fork of the save flown by the activity bot
for a few dockings, with the pack and without it. `crates/app/tests/gm.rs` keeps it so.

| Pack | What's wrong | Rejected at | Code |
|---|---|---|---|
| `silent_owner` | Hands Kettle Dock to another faction on every docking there and never says so | legibility | `silent_change` |
| `late_spin` | Runs fine at first, then spins for ever once the player has docked twice | sandbox | `over_budget`, `quarantined` |

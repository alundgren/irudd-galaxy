# places-lab

A throwaway prototype, like `prototypes/ui-lab`, that settles one question before the
engine builds scenes, cameras and talk: **how does a station become a place you are in?**
Only the findings carry over, in a REPORT.md at the end.

The ui-lab's verdict ("Where the player is" in `prototypes/ui-lab/REPORT.md`): a speech
bubble hangs off a building, not a person; opening the cradle cuts to another scene and
opening the office nudges the camera, and the two don't match. [RESEARCH.md](RESEARCH.md)
has what other games do, checked against sources.

It starts from a copy of the ui-lab (its screens as Luau widget trees, its renderer copy)
and adds a place model, the station's people as figures, and four choices switched live.

## Run

From this folder, on any OS (Windows checked; macOS and Linux unchecked):

```sh
cargo xtask cargo run                                   # the window, 1280x720
cargo xtask cargo run -- --combo cut,staged,bubble,still   # start on a combination
cargo xtask cargo run -- --shots shots                  # the scripted run: PNGs, measures, PASS or FAIL
```

The scripted run goes from the station into each room, hears its person, does the room's
thing, goes back, and round again. It reports what a player would feel: how long each
move takes, the clicks from the station to hearing each person (first visit and again),
whether the speaker's face stayed on screen and uncovered while their line showed (a
rule: the run fails if not), and frame time. Like the ui-lab's, it also hovers
everything the pointer can reach in each saved state and fails on any rule break.
`--shots` takes `--combo` too, so each combination gets its own run.

To see a state that isn't scripted, `--look` plays steps headless and, at each `shot`,
saves a PNG and prints what is on screen as JSON (where the player is, the combination,
the line being said, the people here and their anchors, every target and any rule break):

```sh
cargo xtask cargo run -- --look looks "click key_office_block; arrive; shot office"
cargo xtask cargo run -- --look looks "go market; arrive; click person_juno; shot; key 3; shot"
```

Steps: `hover W`, `click W`, `press W`, `release`, `leave`, `drag W V`, `drop`, `back`,
`wait N`, `arrive` (wait for a move to land), `go PLACE`, `key N` (cycle choice N),
`combo SPEC`, `zoom STEPS [W]`, `shot [NAME]`, `sweep`. `W` is a widget id, a path from
the JSON, or `x,y`.

`cargo xtask` here is the engine's xtask (`.cargo/config.toml`), so the lab builds in the
shared build folders and waits for the build permit like everything else. Tests:
`cargo xtask cargo test`.

## Try

| | |
|---|---|
| Go places | Click a module or a key icon at the bottom. Esc, right-click or Station takes you back out. A click during a move lands it. |
| Talk | The first time you walk into a room its person speaks; click the line for the next. Click anyone in a room to talk to them. |
| Rooms | Ilse's dock office, Juno's market (sell and buy), Orrin's cradle (drag a part from the rack onto a socket), Bel's canteen (buy a bowl). |
| Life | After Ilse's welcome, Pim finishes eating in the canteen and takes a parcel to the market. |
| 1 2 3 4 | Cycle the variant of each choice: getting between places, what a room is, how people talk, life. Shift goes back. The combination is in the bottom right corner. |
| F8 | Write a note: the shot is taken as you press it; type in the window, Enter saves, Esc drops it. Notes go to `notes/notes.md` here (not in git), tagged with the combination, the place and the time, the shot beside them. |
| F3 | The combination, where you are, the last move's time, clicks, screen runs, frame time, and how often the speaker was hidden. |
| F5 | Start again. |

Rule breaks show in a bar along the top, outlined on screen. The screens and the station
are Luau in [`lab/`](lab): save a file and the lab reloads it, keeping the state.

## How it's built

**The place model** (`src/places.rs`), shared by every variant:

- **Places nest:** station > module > interior. A place has a name, a parent, the
  places it connects to, a room it leads into (`enter`), where you arrive in it, its
  floor size and named floor spots (where people stand, the door), so walking can come
  later.
- **People are beings in places:** a name, a role, a place, a spot, a state (at work,
  eating, away) and a home room. The world publishes points their speech and face come
  from. A story moment moves them (`world.move` from the Luau; the life variant decides
  how it shows).
- **The player has a place.** `world.go(place)` moves them at once; the travel variant
  only shows the way. When the view lands, the Luau hears `on.arrive`.
- **Content says what there is:** [`lab/places.luau`](lab/places.luau) declares the
  station's places and people; the screens read the model as `world` (`world.here`,
  `world.place`, `world.people`, `world.people_here`, ...).
- **Screens attach to anchors the world publishes from the model,** never to buildings:
  `person.<id>` (over the head), `person.<id>.face`, `place.<module>` and
  `place.<room>` (seen from outside), `spot.<name>` (a room's floor spots),
  `socket.<name>` (the Wren in the cradle).

**A view** is built each frame: the travel variant composes it (at rest, the place's own
view), the room variant draws interiors, the talk variant may move it, then the screen is
drawn over it. `src/stage.rs` is what the variants draw with: `stage.view(place)`,
`stage.exterior(..)`, `stage.set(place, transform, shown)` (a room's set and its people
anywhere, with their anchors), `stage.set_camera(place)`, `stage.title(place)`.

**The people** (`src/figures.rs`): Ilse (tall, long coat, peaked cap), Orrin (short and
broad, apron, goggles), Bel (round, cook's hat), Juno (slim, wide flat hat, scarf) and
Pim (small, big satchel, bobble cap). Each is a body and a head, so the head can turn
and nod; `life::idle` breathes, looks round and nods while talking.

**The sets** (`src/sets.rs`): each room is a stage open at the front, with its fixed
camera framed so the people land left of the room's panel.

### The four choices, and the files each owns

Each choice is a trait in its own folder. A variant is one new file in that folder and
one line in the folder's `all()`; nothing outside the folder changes, so four agents can
work at once.

| Key | Choice | Trait | Folder (owned by that choice's agent) | Baseline |
|---|---|---|---|---|
| 1 | Getting between places | `Travel`: `start`, `step`, `busy`, `finish`, `compose`, `overlay` | `src/travel/` (`mod.rs`, `cut.rs`) | `cut`: a title card |
| 2 | What a room is | `Room`: `view`, `step`, `widgets` | `src/room/` (`mod.rs`, `staged.rs`, `layers.rs`, `inside.rs`) | `staged`: a staged 3D set, fixed camera. Also `layers`: painted flats in depth, like a toy theatre, with scene text; `inside`: the set in its module on the station, the hull cut away, the core beyond |
| 3 | How people talk | `Talk`: `widgets` (when the screen runs), `frame` (each frame) | `src/talk/` (`mod.rs`, `bubble.rs`) | `bubble`: on the person |
| 4 | Life | `Life`: `moved`, `arrived`, `step`, `pose`, `widgets` | `src/life/` (`mod.rs`, `still.rs`) | `still`: people stay put |

Every trait also has `setup(renderer, world, model)` to upload meshes of its own. A
variant adds widgets with `src/widgets.rs` (`W::new("panel").set("role", "speech")...`),
in the screens' own vocabulary and under the same rules. Everything else (`app.rs`,
`stage.rs`, `world.rs`, `places.rs`, `sets.rs`, `figures.rs`, `combo.rs`, `ui/`, `lab/`)
is shared: change it in an integration pass, not from a variant.

Whatever a talk variant does, the speaker's face must stay on screen and uncovered while
their line shows: the lab checks it every frame.

## The variants still to build

The baselines are in (the table above); these are next:

| Choice | Variants |
|---|---|
| Getting between places | glide in and dissolve into the room · cutaway (the wall opens in place) · a threshold (doorway or short corridor) · honest menu over the dimmed station · walk (an avatar crosses the cutaway) |
| How people talk | portrait with a name panel · the camera frames the speaker · a text column at the side |
| Life | people in states who move at story moments · changes visible when you come back |

Variants are named by technique. Don't copy any game's layout, screens or look.

## Order

1. **Fan out:** one agent per choice, each in its own folder (above), on the integration
   branch `prototype/places-lab`.
2. **Play and report:** the owner plays the combinations and leaves F8 notes; the report
   says what the engine's place model, cameras and talk should be.

# places-lab

A throwaway prototype, like `prototypes/ui-lab`, that settles one question before the
engine builds scenes, cameras and talk: **how does a station become a place you are in?**
Only the findings carry over: [REPORT.md](REPORT.md) has them and the owner's pick.

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
| Life | After Ilse's welcome, Pim finishes eating in the canteen and takes a parcel to the market. With `states` or `return` (key 4) people also have a day: the station's clock moves one step each time you go into a room. Go round the rooms in order to see each round change. |
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
| 1 | Getting between places | `Travel`: `start`, `step`, `busy`, `finish`, `compose`, `overlay` | `src/travel/` (`mod.rs`, `shared.rs`, one file per variant) | `cut`: a title card |
| 2 | What a room is | `Room`: `view`, `step`, `widgets` | `src/room/` (`mod.rs`, `staged.rs`, `layers.rs`, `inside.rs`) | `staged`: a staged 3D set, fixed camera. Also `layers`: painted flats in depth, like a toy theatre, with scene text; `inside`: the set in its module on the station, the hull cut away, the core beyond |
| 3 | How people talk | `Talk`: `widgets` (when the screen runs), `frame` (each frame) | `src/talk/` (`mod.rs`, `bubble.rs`, `portrait.rs`, `framing.rs`, `column.rs`) | `bubble`: on the person |
| 4 | Life | `Life`: `moved`, `arrived`, `step`, `pose`, `widgets`, `world`, `event`, `props`, `settling` | `src/life/` (`mod.rs`, `still.rs`, `day.rs`, `states.rs`, `returning.rs`), `lab/life.luau` | `still`: people stay put |

Every trait also has `setup(renderer, world, model)` to upload meshes of its own. A
variant adds widgets with `src/widgets.rs` (`W::new("panel").set("role", "speech")...`),
in the screens' own vocabulary and under the same rules. Everything else (`app.rs`,
`stage.rs`, `world.rs`, `places.rs`, `sets.rs`, `figures.rs`, `combo.rs`, `ui/`, `lab/`)
is shared: change it in an integration pass, not from a variant.

Whatever a talk variant does, the speaker's face must stay on screen and uncovered while
their line shows: the lab checks it every frame.

## Getting between places (key 1)

Key 1 cycles these, in this order. Each goes both ways with the same grammar (coming out
is going in, played backward), all four rooms match, and a click during a move lands it.
Times are from `--shots` with `--combo <variant>,staged,bubble,still`.

| Variant | What it does | Move time |
|---|---|---|
| `cut` | The baseline: a dip to a title card naming the place. | 1.02 s |
| `glide` | The camera flies at the module while the station falls away into the dark, far parts first; then the room's set comes up into the light layer by layer (the set, then its people) as the camera settles. Out: the room's people sink first, then the set, and the station rises back round the module as the camera pulls out. | 1.27 s |
| `cutaway` | The room stands inside its module, part of the same object. The camera comes close over the module, its shell folds down and sinks into its floor, and there is the room with its people; the camera settles down into the room's own view. Out: the camera rises, the shell closes over the room, the camera pulls back. | 1.52 s |
| `threshold` | One short corridor between any two places: you walk down it to a door with the place's name over it, it slides open on the place's light (lamplight for rooms, starlight for the station) and you step through. The same corridor every time. | 1.27 s |
| `menu` | The honest menu: you never leave the station's view. The room opens as a framed window over the station, which stays as you left it, dimmed round the window and behind the room's screen (`Travel::underlay`). | 0.35 s |
| `walk` | The player is a figure standing at the hub. The camera comes down over the station, the figure walks along the deck and the module's strut to its door and goes in, then a short dip into the room. Out: the dip, the figure steps out of the door and walks back to the hub. The figure is larger than life and walks at a brisk, sped-up pace (24 station units a second). | office 2.90 s in, 2.82 s out; canteen 1.95 / 1.88 s; market 1.72 / 1.63 s; cradle 1.53 / 1.45 s |

**`menu` with the `staged` room.** The staged set still draws: `menu` shows the room as
the room variant shows it, with its camera stood back (1.3 times for rooms up to 9 m
wide) and moved left as far as its people allow, so the station shows behind and to the
right of the room, dimmed, under the room's screen. In the office and the canteen the
station is plainly there behind the screen. In the market (Pim stands at the left) and
the cradle (14 m wide, so it isn't stood back at all, or its socket labels collide) the
room covers most of the screen and the station shows only at the edges. With another
room variant, `menu` keeps that variant's own framing and draws no window or dimmed
station (it can only place the window round the staged set): `layers` fills the screen,
and `inside` already shows the station round the room.

**`glide`, `cutaway` and the room variant.** `glide` lands on whatever the room variant
shows (it dims and raises that view's meshes). `cutaway` builds the move from the room's
set standing in its module and lands on the room variant's own view, so with a room
variant other than `staged` the last frame cuts from the set to that view.

The talk variants, after `bubble`:

| Variant | What it does |
|---|---|
| `portrait` | A bust of the speaker on a card at the bottom left (a small copy of their own figure, just in front of the camera, so it is the same cel-and-ink model), with their name and line beside it. Their figure stays in the room. |
| `framing` | The camera cuts in close on the speaker's face on a longer lens and eases back when the talk ends; the line is a caption at the bottom. It pushes in only as far as keeps what the room hangs on the scene usable, so in the cradle it stays wide. |
| `column` | The talk runs as a column down the left, newest at the bottom, each line with its speaker's name. The speaker's ink turns amber with a ring at their feet. The room moves up or over, smaller if it must, so no face, sign or socket is under the column. |

### Life

| Variant | What it does |
|---|---|
| `still` | People stay where the content put them; a story moment moves them at once, unseen. |
| `states` | People do things in their places, and you see it. Each has a day of acts (`src/life/day.rs`: at work, eating, on a break, visiting, away at lunch), each with a spot and a pose. When the day or a story moment moves someone in the room you walk into, you watch them get up and walk: to another spot, out of the door, or in through it. A story moment out of sight waits up to three room visits for you to come and see it (Pim leaving the canteen after Ilse's welcome). On the station, a module's sign names who is there who doesn't belong (`Canteen · Orrin`); in a room, signs say what people are doing. Someone walking has no sign. |
| `return` | The station moves on behind your back, never in front of you. The same day runs, story moments wait for you to leave, and what you did leaves a detail in the room (your ice stacked in the market, the lens's crate in the cradle, a mug on Bel's counter, the Wren's pennant in the office). Each room is remembered as you left it. On the station, rooms that changed while you were elsewhere carry a `Changed` mark (hover: what changed). Coming in, people here now or doing something else are marked, the room's panel says who went where, and what is new can be clicked (its person says what it is). Whoever has something to say about what you did says it as you come in (Orrin about the part you fitted, Bel about the bowl, Juno about what you sold, Ilse about the berth). |

The life variants share hooks outside `src/life/`: `world.life` (what the Luau sees of
the variant), `event` (what the player did), `props` (meshes a variant adds to a room's
set) and `settling` (the scripted run waits while someone walks where the player can
see). `lab/life.luau` adds the variant's marks to the signs the screens already have.

Variants are named by technique. Don't copy any game's layout, screens or look.


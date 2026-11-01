# places-lab report

**Question.** How does a station become a place you are in? What should the engine's
place model, cameras, talk and life be before it builds scenes on them?

**Short answer.** A place model in the world, with the screens and the camera presenting
it, fixes what the ui-lab found: speech now comes from a person, and every room is
entered the same way. The owner played the combinations and picked
**`cut · staged · bubble · states`**:

| Choice | Pick | What it is |
|---|---|---|
| Getting between places | `cut` | A short dip to a title card naming the place (1.02 s). A placeholder (below). |
| What a room is | `staged` | A real 3D set, open at the front, with its people standing on its floor and a fixed camera framing them left of the room's panel. |
| How people talk | `bubble` | The line in a bubble on the speaker's figure; click for the next. |
| Life | `states` | People have a day of acts with a spot and a pose each, and when that moves them in the room you are in, you watch them get up and walk. |

**Where it goes next.** The cut is temporary. Later the player moves around themselves,
either old-school first person or over the shoulder, and walkable 2.5D corridors join the
rooms. The staged room was picked partly because it suits that. Build now so that
walking later is a change to the camera and to travel, not a rebuild of rooms or of the
place model.

## What the engine should build

### The place model is world state

Where the player is, which places exist and who is in each are game state, owned by the
world and recorded like any other state. The camera, the travel and the screens only
present it. Everything below follows from that.

- **Places nest:** in the lab, station > module > interior. A place has a parent, the
  places it connects to (its doors), the place it leads into, where you arrive in it,
  its floor size and named floor spots (where people stand, the door). The kinds and
  names come from the module, not the engine (the lab declares them in
  `lab/places.luau`). A corridor is later just another place between two others.
- **People are beings in places:** a place, a spot and a state (at work, eating, away),
  plus a home place. A story moment moves them through the world (`world.move`); the
  life presentation decides how the player sees it.
- **The player has a place.** Going somewhere changes it at once; travel only shows the
  way, and the world tells the scripts when the view lands (`on.arrive`). A click during
  a move lands it.
- **What hasn't been seen stays pending.** A story moment that the player hasn't seen is
  kept, not dropped. In the lab, Ilse's welcome repeats until heard to the end, and
  Pim's walk waits up to three room visits for the player to come and see it. If the
  view goes away mid-walk, the walk finishes in the world.

The engine boundary: the model is generic (places in a tree, beings with places, spots
and states, named anchors). Station, module, dockmaster and canteen are module content.

### Rooms are 3D sets with a camera that can move

- Each room is a set built in 3D, its people standing on its floor at their spots. Its
  camera is a pose in that room, not baked into the set. Walking later moves that camera
  and adds a player figure, without rebuilding the room.
- Not carried over: `layers` (painted flats can't be walked) and `inside` (the room seen
  inside the station from outside).
- The set's camera frames people clear of the room's panel. In the lab, framing was the
  most frequent source of breaks: a hat over a face seen from too high, a label under
  the panel at the edge of an idle sway. Screen rules caught each one, so the engine's
  room views go through `check` like screens do.

### Travel: one grammar now, walking later

- Every way between places goes through one travel presentation (start, step, busy,
  finish, compose a view), so the office and the cradle match. The cut is one variant.
  Walking and corridors become another variant, with nothing else changing.
- Coming out uses the same move as going in, played backward.
- Keep `cut` cheap; it is temporary. The other variants (`glide`, `cutaway`,
  `threshold`, `menu`, `walk`) are recorded in the README and are not carried over.

### Talk: speech on people, through anchors

- People publish anchors: `person.<id>` (above the head) and `person.<id>.face`. Places
  publish `place.<id>` and `spot.<name>`, and things publish their own (`socket.<name>`).
  Screens attach to these and never to a building. This matches the engine thread's
  generic named anchors: the place model is one more publisher.
- **A rule for `check`:** while someone's line shows, their face is on screen and
  uncovered. The lab checks it every frame, by geometry, not just by the anchor being on
  screen (a hat once covered a face). It held for 1560 of 1560 frames with a line in the
  scripted run of the pick.
- Talking to someone again is a click on them in the room.

### Life: a day per person, seen when you are there

- Each person has a day of acts, each with a place, a spot and a pose (`src/life/day.rs`).
  When the day moves someone in the room you are in, you watch them walk. Elsewhere it
  happens out of sight.
- On the station, a module's sign names who is there who doesn't belong there (`Canteen
  · Orrin`). In a room, signs say what people are doing. Someone walking has no sign.
- **Open: what drives the clock.** In the lab it steps once each time the player goes
  into a room, as a stand-in. The game decides (world time, events or player actions);
  the engine needs a clock that scripts can advance and that the world records.

## What a player feels, with the pick

From `--shots --combo cut,staged,bubble,states` on Windows:

| | |
|---|---|
| Each move between the station and a room | 1.02 s, both ways, all four rooms |
| From the station to hearing a person, first visit | 1 click, 1.10 s |
| Talking to them again from the station | 2 clicks, 1.15 s |
| The speaker on screen and uncovered | 1560 of 1560 frames with a line |
| Frame build (scene and UI, CPU) | median 0.03 ms, p95 0.04 ms |

## What building it taught

- **The choices break each other, not themselves.** Every variant passed on its own.
  The breaks came from pairs: a room's framing against a talk panel, a travel window
  against a room that frames itself, an idle sway against a speech bubble. The engine's
  `check` should run screens over every place with the talk and room presentation the
  game uses, not one screen in isolation.
- **Rooms need to draw text** (their own panel, signs), so a room presentation adds
  widgets in the screens' own vocabulary and under the same rules.
- **Switching a presentation finishes what is under way.** Leaving `states` mid-walk
  stranded people until the world was given the end of each walk. Presentations hold no
  state the world can't rebuild.

## Not tried

- Sound, which the research says gives a room much of its feel.
- Walking and corridors themselves; the pick only leaves room for them.
- `bubble` and `states` had a short play only.

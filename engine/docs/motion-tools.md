# Motion tools: films, and what they say about motion

A test checks numbers and a screenshot shows one moment; neither shows how something
*moves*. These tools let an agent check motion before a person plays: film a few seconds
frame by frame exactly as the window would draw them, then ask one question at a time.
Each tool answers with a sentence (`reading`), a few numbers, the times of suspicious
moments and one small picture. Full frames are there for a closer look, not as the first
thing to read.

**Small by default.** A tool's report is compact: the reading, which build drew the film
(`builds`), the few numbers that decide the question with their times (a moment is one
line, `t 2.633 (frame 158, tick 79)`), the picture, and `detail`, naming what was left
out. `--detail` gives the whole report; `motion check` always reads the whole report, so
any value in it can be an expectation. Both binaries print JSON with whatever fits on a
line kept on it (a box, a moment), so a report is a screenful, not hundreds of lines.

```sh
galaxy film --modules tests/play/drift --inputs tests/play/films/inputs/route.toml \
    --from 4 --to 34 --out <folder>             # 61 frames at 60 fps, with probes
galaxy-gm motion strip <folder> --along course   # does the course crawl?
galaxy-gm motion check tests/play/films/course.toml
```

The code: filming in `crates/view/src/film.rs` (shared by both binaries) and with other
builds in `crates/view/src/builds.rs`, `galaxy film` in `crates/galaxy/src/film.rs`,
`galaxy-gm film` and `motion` in `crates/gm/src/film.rs`, and the tools in
`crates/motion` (CPU only, no engine crates).

## Which tool for which question

| The question | Tool | Reads | Look at first |
|---|---|---|---|
| Does a pattern that should stand still crawl, jitter or pop along a path (a course, a lane, a border)? | `strip --along course` or `--along x,y;x,y` | pixels | `drift_px_per_frame`, `drifting` |
| Do dotted lines' dots stay put in the world? Fast, and needs no GPU. | `dots` | probes | `drift_px_per_frame`, `lines` |
| How far and how fast did a thing move, and when? (film `--probes-only`, no GPU) | `track --thing x` | probes | `world.displacement`, `world.speed_max_at` |
| How close does it come to another's surface; does it sink in, how deep, how long? | `track --thing x --to y` | probes | `surface.overlap`, `surface.contact` |
| The gap frame by frame round a moment | `film show --thing x --gap y --from t --to t` | probes | the `gap_to` column |
| Does the camera follow smoothly? Does a thing judder (moves on ticks, not frames)? | `track [--thing x]` | probes | `screen.judder`, `camera.judder`, `flags` |
| Does it stop where it should: overshoot, bounce back, settle? How does it brake? | `track --thing x --to y` | probes | `gap.overshoot`, `gap.rebound`, `approach` |
| What happens, in what order; what does the approach look like? | `sheet [--count n] [--at t,t] [--crop x or x,y,w,h]` | pixels | the picture |
| How does it slow down, speed up, overshoot, bounce, as one picture? | `sheet --onion x` | pixels | the picture, `gaps_between_poses` |
| Does anything pop in or out, show for one frame, or flicker? | `flicker [--from t] [--to t] [--near x]` (`--screen` for overlays) | pixels | `events`, the picture |
| Does a turning thing's shading ease, or do whole faces flip between light and shadow? | `tone [--thing x]` | pixels | `hard_steps`, `prominence.max` |
| Is something moving that has no name, or crawling where it should be still? | `flow` (`--along course` too) | pixels | `moving`, `along` |
| What changed against an approved film (another build, before a module change)? Does the followed thing behave differently? | `compare <other film>` (`--vdp` for ColorVideoVDP) | pixels | `flip.worst_frames`, `behaviour`, the picture |
| Should this keep holding, as a test? | `check <spec>` | either | `expectations` |

How to work a motion question:

1. **Film a short span** round the moment (a few seconds; a film of the ticks you care
   about, or `galaxy-gm film <recording> --note <n>` for a playtest note). Use
   `--probes-only` when the question is about where things are, not pixels: it needs no
   GPU and is fast.
2. **Ask the tool that fits** and read its `reading`. The numbers say how much; the
   timestamps say when; `near` says which things were close by.
3. **Look at its one picture.** Then, if something is flagged, `sheet --at <times>`
   (with `--crop <thing>` or `--crop x,y,w,h` for detail) shows just those frames.
4. **For an unfamiliar defect**, start with `sheet` (6 to 12 frames) and `flicker`, then
   read the frames round what they flag. All frames are kept in `frames/`.
5. **When it is fixed, keep it fixed**: a `check` spec with the bound that separated the
   good film from the broken one.

## Filming

A film is a folder: `film.json` (how it was filmed, the things in it and what changed;
small enough to read whole), `frames.jsonl` (a frame a line: time, tick, camera and every
thing's pose and screen position), `lines.jsonl` (a frame a line: every line and dot
drawn), `outlines.json` (each thing's outline on the plane, as collision sees it), `parts.json`
(the models' parts as triangles, and which model each thing wears) and
`frames/00000.png` (lossless, one per frame), plus `motion/` where the tools put their
pictures. `galaxy-gm film show <film>` prints the probes as a table, a frame a
line, for the things and times asked; `--gap <y>` adds each thing's gap to y's surface. Films of format 1, with every frame inside
`film.json`, still read. Each tool names its picture
after its options (`motion/sheet-at-1.2,1.3-crop-ship.png`), so runs with other options
keep theirs; `--image` puts it elsewhere.

- **The clock.** Frame `k` is shown at `k / fps` seconds (60 by default). By then the sim
  has run `floor(k * 30 / fps)` ticks, and the frame is drawn that far between the last
  two (`alpha`), with that presentation time, exactly as `galaxy play` draws at that
  frame rate. So interpolation, the camera's follow and timed effects (the course
  settling, a ring closing) are filmed as the player sees them. `galaxy shot` draws one
  still moment (`alpha` 1, time 0) and can't show any of that. `galaxy shot --crop <thing>`
  writes just the square round one thing, enlarged in whole pixel blocks, for a before and
  after of one thing.
- **Warm-up.** Scenes are built for every frame from ten seconds before the first filmed
  one, so effects that build up over time are already under way; only filmed frames are
  drawn.
- **Probes.** Each frame keeps its tick, `alpha`, time, the world's hash, the
  camera as a matrix (so a tool can project any world point into that frame) and how many
  world units a pixel covers, the zoom, every drawn thing's interpolated pose and screen
  position, and every line the scene drew, with the centre of each dot of a dotted line
  as the renderer places it. `outlines.json` keeps each thing's outline as collision
  measures it (its reach in 128 directions in its own frame: a body's circle, else its
  model's convex footprint, scaled), so `track --to` measures surface to surface. `--probes-only` films without pictures and without a GPU.
- **Changes.** The manifest's `changes` lists what changed in the world during the film,
  by tick: each input (`drift.order_dock down`), each order given, changed or dropped (an
  entity's `engine.steer` mode and target) and each arrival. `track` lists and marks
  them, so a gap that opens again because of a new order isn't read as a bounce. `orders`
  keeps every thing's order when the first frame was filmed, so a tool knows what each
  was ordered to from the start.
- **Zoom** is the player's, not the world's. An inputs file turns the wheel with
  `[[zoom]] tick, steps`; a played session's recording keeps the player's wheel
  (`saves-and-replay.md`), so a film of it has the zoom the player had.
- **From a playtest.** `galaxy-gm film <recording> --note <n>` films three seconds before
  a note and one after (`--before`, `--after`), at the note's frame size, into
  `films/note-0001` beside the recording; or `--from`/`--to` ticks. The launcher runs the
  build that made the recording, as for `audit`, and the world at every note inside the
  film is checked against the hash the note kept: a film of a different world exits 5
  (`replay_diverged`) rather than pass for what the player saw.
- **Which build drew it.** The world can replay identically on a newer build while it
  draws differently: a drawing defect fixed since shows as fixed. So `film.json` keeps
  `builds` (the build that recorded the session, the one that drew the film, whether they
  are the same, and a note), and `galaxy-gm film`, `film show` and every tool's report
  repeat it. When they differ the note says so plainly, that the film may not look like
  what the player saw, and gives the command that films it with the recording's build.
  `--as-recorded` draws with the recording's build; `--builds a,b` films with two
  builds and compares them (below).
- **Filming with other builds.** Installed builds sit side by side, and the build that
  recorded a session is often older than the one with the newest tools, so the current
  `galaxy-gm film` (and `galaxy film` for a scripted run) runs another build's executable
  to draw. Older builds will always lack newer commands and options, so they are asked
  only for what every build since films began takes (`film` with `--note`, `--before`,
  `--after`, `--from`, `--to`, `--fps`, `--size`, `--out`, `--probes-only`). The current
  build then brings the film up to its own format (`crates/view/src/builds.rs`): it writes
  it as format 2, stamps `builds` with the build that drew it, and adds what the older
  build didn't record (the film's changes, things' reach and morph, outlines) from a
  probes-only replay of the same span on itself. The world is the same on both (the
  hashes say so), so those probes are too; the pictures and camera stay the other
  build's. `--builds a,b` (each `recorded`, `this`, a build's name or a part only one has,
  such as its commit) films into `<out>/<build>` each and runs `compare`; any tool can then
  be run on each, as with drift's tight circle: `tone` finds 6 hard steps on the build
  the playtest was recorded on and none on the current one. The launcher doesn't route
  `film` to the recording's build for this reason.
- **Size.** 960x540 by default: a three-second film is roughly 50 MB of PNGs. Films go
  where `--out` says, outside the repository.

**Why frames, not video.** Video codecs are lossy and work across time: they smear fine
detail, add blocking and can make still dots shimmer, which the tools would then measure.
Frames are lossless, line up one to one with the manifest, and open at random. Film and
render-test pipelines keep image sequences as their masters for the same reasons.

## How each tool works, and what it misses

**Camera compensation** (`flicker`, `flow`, `sheet --onion`). The camera follows the
ship, so the whole world slides across the screen every frame. These tools move the
earlier frame into the later one's view through both cameras, for points on the ground
plane: exact for an orthographic camera at any height, close for a perspective one near
the plane. What is still in the world then lines up; what moved, popped or flickered is
left. Something fixed on screen (an overlay) moves exactly against the camera: use
`--screen`, which compares pixels where they are. The backdrop scrolls with a share of
the camera's step (parallax); `flow` fits that share and sets it aside.

- **`track`** reads the probes. On screen: the offset from the centre (for the followed
  thing, the camera's tracking error) and the speed. Judder compares each frame's step
  with the mean of the steps either side: smooth motion, even accelerating, scores near
  0; a thing drawn at its tick position at 60 fps steps 0, 2, 0, 2 and scores about 1 or
  more. With `--to`, the gap to another thing: the closest, `overshoot` (how far past it
  along the way it came), `rebound` (how far it came back after its closest) and
  `settled_at` (within the tolerance of where it ends for at least half a second). When
  an input or order falls between the closest moment and the furthest rebound, the
  reading says so and `gap.rebound_after` names it. The chart marks the film's changes
  in blue. Collisions legitimately jump; they show as single flags, not as judder.
  The camera's step is where the frame before's target lands in this frame, measured at
  the centre, so a zoom doesn't sweep it. Zooms (the player's wheel) are found from the
  camera's scale changing, a run of them a quarter second apart as one, listed in `zooms`
  and marked on the chart; camera and screen steps on a zoom's frames are set aside, not
  flagged (`zoom_steps_set_aside`). With `--to`, `approach` tells how it came in: top
  speed, the last moment at it (`braking_from`), its speed when the gap was 75, 50, 25 and
  10% of the start, when it stopped (under 2% of the top speed), the turn it then made on
  the spot, and when `engine.arrived` came. Also with `--to`, `surface`: the gap between
  the two outlines along the line between their centres, as collision measures it
  (`crates/sim/src/shape.rs`), from `outlines.json` and each frame's pose: the closest,
  the gap when it arrived (else at the end), each **overlap** (a run of frames with the
  gap below -0.0001: a thing resting against or sliding along another sits at 0, give or
  take a millionth of rounding) with its deepest, the total time and how many, and each
  run **within contact range** (gap up to 0.05, overlapping or not). Gaps under 0.1 keep
  three significant figures, so a shallow overlap doesn't round to 0. Overlaps on frames
  drawn on a tick are the sim's own; on frames between ticks only, the drawn path cuts
  across as the thing slides round a curved surface (ramming drift's hub: 8 one-frame
  overlaps, deepest -0.0155, all between ticks). A mining ship holding at a rock comes to
  0.005, within contact range with no overlap. `frames_round_it` is the `film show
  --gap` command that lists the gap frame by frame round the first one. When the other
  thing is the target of the thing's own order (from `orders` and the changes; an
  arrival there keeps it), `order_target` names the order, each overlap run then says
  `ordered_to_it` and the reading says so: a ship docking onto its pad overlaps it by
  design. `first_contact` is the first run within contact range and its `bounce`: how
  far the surfaces came apart right after it, up to their first peak and before touching
  again (`contact_again_at`). `gap.rebound` counts all the centres' opening after the
  closest moment; under held thrust that includes flying off round the other thing
  later, so the bounce is the number for "did it bounce".
  In the world, `displacement` is start to end in a straight line, `distance` the path,
  and `speed_max_at` when the top speed came.
- **`dots`** matches each fully visible dot with the nearest of the frame before, line by
  line (the leg being flown can slide while later legs stand still). A line is followed
  from frame to frame by where it is drawn from, so a flight of several legs is measured
  leg by leg, and only a line seen for a quarter second or more decides. Pixels are at the
  camera's scale, before the tilt shortens them on screen. Dots without a match popped;
  where the lines drawn changed within a quarter second of a change the film lists (a leg
  finished, a new order), the pops are marked `with` it, as expected.
- **`strip`** is a kymograph: it samples a narrow band along a path fixed in the world,
  projected through each frame's camera, and stacks the samples, time down. Still things
  draw vertical stripes and sliding ones slanted stripes. The shift between rows comes
  from correlating each row with the one before after taking out slow brightness changes,
  so an intended fade is not movement. `--along course` takes the leg being flown, from
  just inside its far end to short of where the ship gets to, so neither the ring nor the
  ship passes over it. A periodic pattern can only be followed up to half its period a
  frame; faster, it aliases (the report warns when steps get close).
- **`sheet`** lays frames side by side with their time and tick: `--at` alone shows just
  those frames, `--count` spreads frames evenly, and both together show both. `--crop`
  cuts round a thing, or a box `x,y,w,h`, at full size. `--onion` draws a few poses over each
  other in a window fixed in the world, blue (early) to red (late), with the path: even
  spacing is steady speed, shrinking spacing is slowing, a pose past the target is an
  overshoot.
- **`flicker`** compares each frame with the one before (and before that), compensated,
  as the mean brightness round each pixel: edges re-drawn a fraction of a pixel apart
  move light between neighbours without changing the sum, so edge shimmer doesn't count.
  Frames are smoothed by about a pixel first: moving a frame into another's view samples
  between pixels, which blurs by an amount that depends on the fraction of a pixel the
  camera moved. Unsmoothed, a camera moving 4.5 px a frame made every sharp edge blurred,
  sharp, blurred, and the hub on drift's dock film showed 20 "flicker runs" that weren't
  on screen; smoothed, 11, all inner ink.
  A cell's change is judged against the busiest of it and its neighbours on the frames
  round it: a **pop** is a jump far above that which stays, a **blip** is back the next
  frame, a **flicker** goes back and forth four frames or more. Something moving into a
  quiet cell is not a pop (its neighbour was busy); a sudden change right beside a moving
  thing can be missed for the same reason. Flagged cells that touch (eight ways) and
  start changing on the same frame or the next are one event, as strong as their changes
  together: one line breaking across a cell edge is one event, not four. Runs that only
  overlap in time stay apart, so events keep their place. Events are ranked strongest
  first; `--from`/`--to` (seconds) and `--near <thing>` (within its reach on screen, plus
  24 px) keep some, and `left_out` says how many they and the cap left out. With `--near`,
  events with a third or more of their changed pixels on the thing's model (from the
  part map below) are listed first, and each says whether it is `on` the model or only
  near it: a course line crossing near a hub is not the hub's. The whole
  film is read either way, so a cell's usual change is known at the edges of the span.
  The picture is a motion history: where things changed, coloured by how recently, with
  the flagged cells boxed. The three strongest events each get a close-up
  (`closeup`), and `--event <n>` (each event's `n`; then the report is that event alone)
  or `--at <t>` (the event starting
  there, else with `--near` the box round that thing) makes one for any other: the frame
  before moved into the view of the frame it changed on, that frame, and the changed
  pixels (warm brighter, blue darker) over it dimmed, enlarged in whole pixel blocks
  (nearest neighbour); `changed` says how many pixels changed and which thing is nearest
  them. The move is exact on the ground plane only, so the close-up first lines the box
  up by a shift of up to four pixels: a tall thing (a tower, a rim) slides a little
  against the camera's move.
  Every listed event says what its change **looks like**, from its difference mask once
  lined up: `core_share` is the share of changed pixels whose eight neighbours changed
  too (a filled patch has a core, a band a pixel or two wide has none) and `edge_share`
  the share within two pixels of an edge in both frames (an outline that stepped is an
  edge before and after; a line that appeared is one only after). A core of 30% or more
  is an area appearing, vanishing or changing tone; else 60% or more at edges in both is
  edges stepping; else a thin line appearing or breaking. And it says which model
  **parts** (glTF nodes) lie under it: the film's `parts.json` keeps every model's parts
  as triangles and which model and scale each thing wears, so the tool draws the parts
  into a small part map of the box (nearest the camera wins) and gives each changed
  pixel to the part nearest the camera within two pixels, as a part's ink outline lies
  just outside it, over what is behind. On drift's dock film the hub's events read as
  edges stepping on its towers, drum and ring; the course appearing reads as a thin line
  on no model. `galaxy-gm model <module> <model>` lists a model's parts with their sizes. It can only see what
  the film shows: below 60 fps or 1280x720 it adds `warnings`, as thin lines and
  one-frame events can fall between frames or pixels (a hub's broken inner ink line shows
  at 60 fps and the player's size, and was missed at 30 fps and 960x540).
- **`tone`** follows one thing's shading, which `flicker` sets aside because a turning
  thing is a busy place. Each frame it takes the pixels inside the thing's faces: in a
  disc of the thing's reach (from the probes), not the sky's colour (the median of a ring
  just outside the disc) and flat (no neighbour more than 0.03 brighter or darker: edges
  and ink lines move as it turns). Their brightness histogram ignores where pixels are,
  so turning barely changes it, while a face changing tone shifts it. `change` is the
  earth mover's distance between one frame's histogram and the last's: the mean
  brightness shift of the faces. A face easing over eight frames spreads the shift over
  them; a hard cel step puts it in one frame. So a step is a frame whose change is clear
  (0.01 or more) and five times the mean change of the frames round it (eight either
  side, leaving out the next ones, as a step can land on two frames). Band shares (the
  dark, middle and light thirds) are drawn but not used to find steps: a flat face
  crosses any band edge in one frame even while it fades. A thing that is mostly
  background colour, or smaller than a few dozen flat pixels, can't be measured.
- **`flow`** is pyramidal Lucas–Kanade optical flow on a grid of textured points, between
  compensated frames: at each point, the small shift that best explains how the picture
  round it changed, found coarse to fine. Points still in the world or moving with the
  backdrop are set aside; cells moving steadily otherwise are reported, with the things
  near them. `--along` measures the flow along a path. Something anchored to the followed
  thing moves exactly like the screen and is set aside with the backdrop; that is what
  `--along`, `strip` and `dots` are for.
- **`compare`** scores each pair of frames with FLIP (`crates/motion/src/flip.rs`, our
  implementation of NVIDIA's published LDR-FLIP: colour differences filtered by what the
  eye resolves at the viewing distance, raised where edges or points differ). A frame
  differs visibly when 16 or more pixels score 0.5 or more; a mean over the frame would
  hide a small thing that changed. The picture is the worst frame's error map and a
  distogram (time down, across the screen left to right). FLIP compares stills: run per
  frame it says where and when two films differ, not whether motion looks right, and a
  difference is not a defect (the approved film may carry the flaw). When both films
  follow the same thing, `behaviour` sets how it moved in each side by side (path length,
  top speed and when, arrival, where it ended) and says whether it is the same, so one
  command answers "does it behave differently" as well as "does it look different";
  `film --builds` reports it too. Any two films get `behaviour`: when one is probes only
  or the sizes differ, pixels are skipped and `pixels` says why, so a module before and
  after a change can be filmed `--probes-only` twice and compared. `--vdp` adds
  ColorVideoVDP, a model of how the eye sees differences in video over space and time:
  its score is in JOD, where 10 is no visible difference.
- **`check <spec>`** films what a spec says and runs its expectations: each names a tool,
  a value in its report (`gap.overshoot`, dotted) and bounds; any other key is the tool's
  option. Each tool runs once for the same options: `runs` gives its reading and picture
  once, and `expectations` a short line each (`run`, the value, bounds, `got`, `pass`).
  It exits 5 (`motion_expectations`) when one does not hold. The format is in
  `crates/motion/src/check.rs` (`galaxy-gm fields spec` lists it); `tests/play/films/`
  has three. Without `--out` the film goes to `galaxy-motion-check/<spec>` in the OS temp
  folder, and the report's `film` says where.

## What they caught on drift

Each tool was run on drift with the engine as it is and with a defect put back on
purpose (never committed):

| Defect put in | Tool | As it is | Broken |
|---|---|---|---|
| Course measured from the ship (the reeled-in chain) | `strip --along course` | +0.001 px a frame, confidence 0.998 | -2.74 px a frame in 60 of 60 steps |
| | `flow --along course` | +0.002 | -2.74 |
| | `dots` | 0.000 | -3.15 (at the camera's scale) |
| | `compare` | identical: 0 frames differ | 61 of 61 frames, the course boxed; ColorVideoVDP 8.87 JOD |
| Camera follows the tick position, ignoring `alpha` | `track` | camera judder 0.02 | screen judder 1.98, camera 1.62, ship up to 6.9 px off centre |
| Docking arrives too fast | `track --to berth` | overshoot 0, settles at 2.35 s | overshoot 5.05, rebound 4.25, not settled |
| | `sheet --onion ship` | poses 9.0, 13.2, 2.6, then 0 apart | 9.0, 14.0, 6.8, 8.2, 3.1, 3.2: past the berth and back |
| Course snaps faint instead of settling | `flicker` | no event at 2.55 s | pops along the course at 2.55 s |
| Course drawn on every other frame | `flicker` | none | 50 flicker runs |
| The two course defects | `check course.toml`, `check dock.toml` | pass | exit 5, naming the values out of bounds |
| Faces step between tone bands (`[light] band_fade_degrees = 0`), ship in a tight circle | `tone` | 0 hard steps; at most 3.1 times the change round it | 4 to 5 hard steps, 7 to 10 times, about every 0.75 s; same at 960x540 and 1280x720 |
| | `check tone.toml` | pass | exit 5 |
| | `flicker` | 37 flicker runs, none near the ship | the same 37: the turning ship is a busy place it sets aside |

On the engine as it is, `flicker` also finds real small things: the leg after a waypoint
brightens at once when the ship reaches it, and the hub's inner ink breaks and joins as
the camera moves (`flicker --near hub` on the dock film at 60 fps and 1280x720: 11
flicker runs; with inner ink switched off, 1). Inner ink is found in a single-sample
geometry buffer while colour is multisampled, so a crease or paint border a pixel wide
steps from pixel to pixel as it moves. Dropping facets under a pixel (`too_small` in
`mesh.wgsl`) plays no part: without it the count is the same. Read such findings by
their `change` (brightness times pixels): a few units is a detail, tens is plain to see.

## Alternatives and why this design

- **Watching video, or sending it to a video model.** Dense motion costs many images of
  context, and video models sample sparsely (often one frame a second), which misses
  brief flicker and interpolation faults entirely. Measuring the question at hand gives
  numbers and times from every frame. A video-model second opinion may come later, for a
  short, explicitly framed question, checked first against known broken films.
- **Sim positions only.** Free, but they can't see interpolation, the camera or anything
  the shaders do. Probes are taken after interpolation and projection, and the pixel
  tools read what was drawn.
- **Screenshots every few ticks.** Misses interpolation, and samples can alias: a pattern
  crawling one dot spacing per sample looks still. Every frame at display rate avoids
  both.
- **Libraries (OpenCV, a FLIP package) or Python tools.** The analysis is ours, in Rust,
  as learning is a goal and the GM must carry it to any machine with no Python. Only
  ColorVideoVDP stays outside: it needs PyTorch, so `compare --vdp` writes its script to
  the temp folder and runs it with `uv`, which fetches what it needs on first use, and
  says so plainly when `uv` is missing.
- **Frame timing.** Films are drawn offline and say nothing about real frame delivery,
  vsync or stalls. The window's frame-time report (`galaxy play` prints it) is the
  measure for pacing.

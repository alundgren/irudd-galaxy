# Motion tools: films, and what they say about motion

A test checks numbers and a screenshot shows one moment; neither shows how something
*moves*. These tools let an agent check motion before a person plays: film a few seconds
frame by frame exactly as the window would draw them, then ask one question at a time.
Each tool answers with a sentence (`reading`), a few numbers, the times of suspicious
moments and one small picture. Full frames are there for a closer look, not as the first
thing to read.

```sh
galaxy film --modules tests/play/drift --inputs tests/play/films/inputs/route.toml \
    --from 4 --to 34 --out <folder>             # 61 frames at 60 fps, with probes
galaxy-gm motion strip <folder> --along course   # does the course crawl?
galaxy-gm motion check tests/play/films/course.toml
```

The code: filming in `crates/view/src/film.rs` (shared by both binaries), `galaxy film`
in `crates/galaxy/src/film.rs`, `galaxy-gm film` and `motion` in `crates/gm/src/film.rs`,
and the tools in `crates/motion` (CPU only, no engine crates).

## Which tool for which question

| The question | Tool | Reads | Look at first |
|---|---|---|---|
| Does a pattern that should stand still crawl, jitter or pop along a path (a course, a lane, a border)? | `strip --along course` or `--along x,y;x,y` | pixels | `drift_px_per_frame`, `drifting` |
| Do dotted lines' dots stay put in the world? Fast, and needs no GPU. | `dots` | probes | `drift_px_per_frame`, `lines` |
| Does the camera follow smoothly? Does a thing judder (moves on ticks, not frames)? | `track [--thing x]` | probes | `screen.judder`, `camera.judder`, `flags` |
| Does it stop where it should: overshoot, bounce back, settle? How close does it get? | `track --thing x --to y` | probes | `gap.overshoot`, `gap.rebound`, `gap.settled_at` |
| What happens, in what order; what does the approach look like? | `sheet [--count n] [--at t,t] [--crop x]` | pixels | the picture |
| How does it slow down, speed up, overshoot, bounce, as one picture? | `sheet --onion x` | pixels | the picture, `gaps_between_poses` |
| Does anything pop in or out, show for one frame, or flicker? | `flicker` (`--screen` for overlays) | pixels | `events`, the picture |
| Is something moving that has no name, or crawling where it should be still? | `flow` (`--along course` too) | pixels | `moving`, `along` |
| What changed against an approved film (another build, before a change)? | `compare <other film>` (`--vdp` for ColorVideoVDP) | pixels | `flip.worst_frames`, the picture |
| Should this keep holding, as a test? | `check <spec>` | either | `expectations` |

How to work a motion question:

1. **Film a short span** round the moment (a few seconds; a film of the ticks you care
   about, or `galaxy-gm film <recording> --note <n>` for a playtest note). Use
   `--probes-only` when the question is about where things are, not pixels: it needs no
   GPU and is fast.
2. **Ask the tool that fits** and read its `reading`. The numbers say how much; the
   timestamps say when; `near` says which things were close by.
3. **Look at its one picture.** Then, if something is flagged, `sheet --at <times>`
   (with `--crop <thing>` for detail) shows those frames beside even-spaced ones.
4. **For an unfamiliar defect**, start with `sheet` (6 to 12 frames) and `flicker`, then
   read the frames round what they flag. All frames are kept in `frames/`.
5. **When it is fixed, keep it fixed**: a `check` spec with the bound that separated the
   good film from the broken one.

## Filming

A film is a folder: `film.json` (the manifest) and `frames/00000.png` (lossless, one per
frame), plus `motion/` where the tools put their pictures.

- **The clock.** Frame `k` is shown at `k / fps` seconds (60 by default). By then the sim
  has run `floor(k * 30 / fps)` ticks, and the frame is drawn that far between the last
  two (`alpha`), with that presentation time, exactly as `galaxy play` draws at that
  frame rate. So interpolation, the camera's follow and timed effects (the course
  settling, a ring closing) are filmed as the player sees them. `galaxy shot` draws one
  still moment (`alpha` 1, time 0) and can't show any of that.
- **Warm-up.** Scenes are built for every frame from ten seconds before the first filmed
  one, so effects that build up over time are already under way; only filmed frames are
  drawn.
- **Probes.** Each frame entry keeps its tick, `alpha`, time, the world's hash, the
  camera as a matrix (so a tool can project any world point into that frame) and how many
  world units a pixel covers, the zoom, every drawn thing's interpolated pose and screen
  position, and every line the scene drew, with the centre of each dot of a dotted line
  as the renderer places it. `--probes-only` films without pictures and without a GPU.
- **Zoom** is the player's, not the world's. An inputs file turns the wheel with
  `[[zoom]] tick, steps`; a played session's recording keeps the player's wheel
  (`saves-and-replay.md`), so a film of it has the zoom the player had.
- **From a playtest.** `galaxy-gm film <recording> --note <n>` films three seconds before
  a note and one after (`--before`, `--after`), at the note's frame size, into
  `films/note-0001` beside the recording; or `--from`/`--to` ticks. The launcher runs the
  build that made the recording, as for `audit`.
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
  `settled_at` (within the tolerance of where it ends for at least half a second).
  Collisions legitimately jump; they show as single flags, not as judder.
- **`dots`** matches each fully visible dot with the nearest of the frame before, line by
  line (the leg being flown can slide while later legs stand still). Pixels are at the
  camera's scale, before the tilt shortens them on screen. Dots without a match popped.
- **`strip`** is a kymograph: it samples a narrow band along a path fixed in the world,
  projected through each frame's camera, and stacks the samples, time down. Still things
  draw vertical stripes and sliding ones slanted stripes. The shift between rows comes
  from correlating each row with the one before after taking out slow brightness changes,
  so an intended fade is not movement. `--along course` takes the leg being flown, from
  just inside its far end to short of where the ship gets to, so neither the ring nor the
  ship passes over it. A periodic pattern can only be followed up to half its period a
  frame; faster, it aliases (the report warns when steps get close).
- **`sheet`** lays frames side by side with their time and tick, even-spaced plus any
  `--at`; `--crop` cuts round a thing at full size. `--onion` draws a few poses over each
  other in a window fixed in the world, blue (early) to red (late), with the path: even
  spacing is steady speed, shrinking spacing is slowing, a pose past the target is an
  overshoot.
- **`flicker`** compares each frame with the one before (and before that), compensated,
  as the mean brightness round each pixel: edges re-drawn a fraction of a pixel apart
  move light between neighbours without changing the sum, so edge shimmer doesn't count.
  A cell's change is judged against the busiest of it and its neighbours on the frames
  round it: a **pop** is a jump far above that which stays, a **blip** is back the next
  frame, a **flicker** goes back and forth four frames or more. Something moving into a
  quiet cell is not a pop (its neighbour was busy); a sudden change right beside a moving
  thing can be missed for the same reason. The picture is a motion history: where things
  changed, coloured by how recently, with the flagged cells boxed.
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
  difference is not a defect (the approved film may carry the flaw). `--vdp` adds
  ColorVideoVDP, a model of how the eye sees differences in video over space and time:
  its score is in JOD, where 10 is no visible difference.
- **`check <spec>`** films what a spec says and runs its expectations: each names a tool,
  a value in its report (`gap.overshoot`, dotted) and bounds; any other key is the tool's
  option. It exits 5 (`motion_expectations`) when one does not hold. The format is in
  `crates/motion/src/check.rs`; `tests/play/films/` has two.

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

On the engine as it is, `flicker` also finds real small things: the leg after a waypoint
brightens at once when the ship reaches it, and the hub's inner ink line breaks and joins
as the camera moves. Read such findings by their `change` (brightness times pixels): a
few units is a detail, tens is plain to see.

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

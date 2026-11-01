//! The motion tools, `galaxy-gm motion <tool> <film>`.

use super::Command;
use crate::discover::Tool;

const IMAGE: (&str, &str) = (
    "--image <file.png>",
    "where the picture goes; by default <film>/motion/<tool>-<options>.png, named after the \
     options so runs with other options keep theirs",
);

pub static COMMANDS: &[Command] = &[
    Command {
        tool: Tool::Gm,
        path: "motion track",
        synopsis: "<film> [--thing <x>] [--to <y>] [--tolerance <units>]",
        answers: "How does one thing move: does the camera follow it smoothly, does it judder \
                  (move on ticks, not frames), does it overshoot, bounce back and settle where \
                  it should?",
        about: "Reads the probes only, so a probes-only film works. Judder compares each \
                frame's step with the mean of the steps either side: smooth motion, even \
                speeding up, scores near 0; a thing drawn at its tick position at 60 fps steps \
                0, 2, 0, 2 and scores about 1 or more. Under 0.1 is smooth, over 0.4 judders. \
                With --to, the gap to the other thing: overshoot is how far it went past the \
                other thing along the way it came, rebound how far the gap opened again after \
                its closest. A new input or order in the film (`changes`) can explain a \
                rebound: the reading says when one comes after the closest point.",
        args: &[
            ("<film>", "a film folder"),
            (
                "--thing <x>",
                "the thing to follow, by ID or bare name (ship for drift.ship); \
              the one the camera follows by default",
            ),
            ("--to <y>", "measure the gap to this thing"),
            (
                "--tolerance <units>",
                "how near its end the gap must stay to count as settled, \
              world units; 2% of the starting gap (at least 0.25) by default",
            ),
            IMAGE,
        ],
        output: &[
            ("reading", "one sentence"),
            (
                "thing, followed",
                "the thing, and whether the camera follows it",
            ),
            ("span", "the first and last frame: frame, t (seconds), tick"),
            ("world.distance", "how far it went, world units"),
            ("world.speed_max, speed_end", "world units a second"),
            (
                "world.judder",
                "unevenness of its world steps, per pixel (0 smooth, 1 or more \
              judders)",
            ),
            (
                "screen.offset_px",
                "distance from the screen centre in pixels: mean, max, \
              max_at (for the followed thing, the camera's tracking error)",
            ),
            (
                "screen.speed_px_per_frame",
                "on screen: median and max pixels a frame",
            ),
            (
                "screen.wobble_px",
                "each step against its neighbours' mean, pixels",
            ),
            (
                "screen.judder, camera.judder",
                "unevenness on screen and of the camera itself",
            ),
            (
                "flags",
                "moments a step is far off its neighbours': frame, t, tick, kind",
            ),
            (
                "changes",
                "inputs and orders that came during the film: t, tick, kind (input \
              or order), what",
            ),
            ("gap.start, end, closest", "the gap to --to, world units"),
            ("gap.closest_at", "when it was closest"),
            (
                "gap.overshoot",
                "how far past the other thing along the way it came",
            ),
            (
                "gap.rebound",
                "how far the gap opened again after its closest",
            ),
            (
                "gap.rebound_after",
                "an input or order between the closest moment and the \
              rebound, if any: the rebound may be that, not an overshoot",
            ),
            (
                "gap.settled_at",
                "when it came within tolerance of where it ends and stayed \
              for half a second; null if it never did",
            ),
            (
                "image",
                "a chart over time: offset or screen position, speed, step \
              unevenness and the gap; red marks are flags, blue marks inputs and orders",
            ),
        ],
        writes: "<film>/motion/track-<options>.png",
        example: "galaxy-gm motion track films/dock --to berth",
        next: "galaxy-gm motion sheet <film> --onion <thing> to see the approach as one \
               picture; sheet --at <t> for flagged moments.",
        topic: "motion",
    },
    Command {
        tool: Tool::Gm,
        path: "motion dots",
        synopsis: "<film>",
        answers: "Do the dots of dotted lines (a course, a lane) stay put in the world, or do \
                  they crawl or pop?",
        about: "Reads the probes only; needs no GPU. Each fully visible dot is matched with \
                the nearest dot of the frame before. Pixels are at the camera's scale, before \
                the tilt shortens them on screen.",
        args: &[("<film>", "a film folder"), IMAGE],
        output: &[
            ("reading", "one sentence"),
            (
                "drift_px_per_frame",
                "how far dots move along their line a frame, pixels; 0 is \
              still in the world",
            ),
            ("drift_px_per_second", "the same, a second"),
            (
                "moving_steps, steps",
                "frame steps where dots moved, of all steps",
            ),
            (
                "lines",
                "per dotted line: px_per_frame, moving_steps, steps",
            ),
            (
                "popped",
                "frames where dots appeared or vanished instead of fading",
            ),
        ],
        writes: "<film>/motion/dots-<options>.png",
        example: "galaxy-gm motion dots films/route",
        next: "galaxy-gm motion strip <film> --along course to see it in the pixels.",
        topic: "motion",
    },
    Command {
        tool: Tool::Gm,
        path: "motion strip",
        synopsis: "<film> [--along course | --along x,y;x,y | --screen x,y;x,y]",
        answers: "Does a pattern that should stand still (a course, a lane, a border) crawl, \
                  jitter or pop along a path?",
        about: "A kymograph: the pixels along the path in every frame, stacked, time down. \
                Still things draw vertical stripes, sliding ones slanted stripes. The shift \
                between rows is found by correlation after taking out slow brightness changes, \
                so a fade is not movement. A periodic pattern can be followed up to half its \
                period a frame; the reading warns near that.",
        args: &[
            ("<film>", "a film folder with pictures"),
            (
                "--along course",
                "the course leg being flown, from just inside its far end to \
              short of the ship (the default)",
            ),
            ("--along x,y;x,y", "a path fixed in the world, world units"),
            (
                "--screen x,y;x,y",
                "a path fixed on screen, pixels from the top left, for \
              overlays",
            ),
            (
                "--width <px>",
                "pixels either side of the path averaged; 2 by default, at most \
              8",
            ),
            (
                "--max-shift <px>",
                "the largest shift a frame looked for; half the pattern's \
              period by default",
            ),
            IMAGE,
        ],
        output: &[
            ("reading", "one sentence"),
            (
                "drift_px_per_frame",
                "median shift along the path a frame, pixels; positive is \
              `positive_is`",
            ),
            (
                "drift_px_per_second, total_drift_px",
                "the same a second, and in all",
            ),
            (
                "moving_steps, steps",
                "frame steps that moved, of the steps followed",
            ),
            ("confidence", "median correlation of the matches, 0 to 1"),
            (
                "drifting",
                "runs of three or more moving frames: from, to, px_per_frame",
            ),
            ("pops", "frames that break from the one before"),
            ("absent", "spans where the path shows nothing to follow"),
            (
                "period_px, length_px",
                "the pattern's period and the path's length, pixels",
            ),
        ],
        writes: "<film>/motion/strip-<options>.png",
        example: "galaxy-gm motion strip films/route --along course",
        next: "galaxy-gm motion sheet <film> --at <t> for frames it flags.",
        topic: "motion",
    },
    Command {
        tool: Tool::Gm,
        path: "motion sheet",
        synopsis: "<film> [--at t,t | --count <n>] [--crop <thing | x,y,w,h>] [--onion <thing>]",
        answers: "What happens, in what order? Frames side by side, each labelled with its \
                  time and tick; or an onion skin of one thing's poses.",
        about: "--at shows exactly the frames asked for; --count spreads frames evenly; with \
                both, the even ones and the asked ones (outlined in red). --onion draws one \
                picture instead: a few poses of a thing over each other in a window fixed in \
                the world, blue (early) to red (late), with its path; even spacing is steady \
                speed, shrinking spacing slowing down, a pose past the target an overshoot.",
        args: &[
            ("<film>", "a film folder with pictures"),
            (
                "--at <t,t>",
                "the frames at these times, seconds (the t other tools report)",
            ),
            (
                "--count <n>",
                "frames spread evenly over the film, 1 to 60; 12 when --at is \
              not given",
            ),
            (
                "--crop <thing>",
                "cut each frame round a thing at full size, --crop-size pixels \
              square",
            ),
            (
                "--crop <x,y,w,h>",
                "or cut this box, pixels from the top left",
            ),
            (
                "--crop-size <px>",
                "the square round a thing; 256 by default",
            ),
            ("--columns <n>", "frames a row; 4 by default"),
            ("--onion <thing>", "an onion skin of this thing instead"),
            (
                "--poses <n>",
                "poses in the onion skin, 2 to 12; 5 by default",
            ),
            IMAGE,
        ],
        output: &[
            ("reading", "one sentence, naming the frames folder"),
            (
                "frames",
                "each frame shown: frame, t, tick, asked (from --at)",
            ),
            ("crop", "what was cropped round"),
            (
                "poses, gaps_between_poses",
                "with --onion: each pose's time, and the world \
              distance between poses in order",
            ),
            ("image", "the sheet"),
        ],
        writes: "<film>/motion/sheet-<options>.png (onion: sheet-onion-<thing>.png)",
        example: "galaxy-gm motion sheet films/note-0001 --at 1.2,1.25,1.3 --crop ship",
        next: "Full frames are in <film>/frames/; open the ones that matter.",
        topic: "motion",
    },
    Command {
        tool: Tool::Gm,
        path: "motion flicker",
        synopsis: "<film> [--screen]",
        answers: "Does anything pop in or out, show for one frame, or flicker? Whole-picture \
                  sudden changes, with where and when.",
        about: "Each frame is compared with the one before (and before that), moved into its \
                view so the camera's follow cancels out (`compensated`), on a grid of cells, as \
                the mean brightness round each pixel, so edge shimmer doesn't count. A cell's \
                change is judged against the busiest of it and its neighbours on the frames \
                round it. A pop jumps and stays; a blip is back the next frame; a flicker goes \
                back and forth four frames or more. Things moving fast or turning are busy \
                cells, so a change on a turning thing can be missed.\n\
                It sees what the film shows: film at 60 fps and the player's size, or thin \
                lines and one-frame events can be missed (`warnings` says so).",
        args: &[
            ("<film>", "a film folder with pictures"),
            (
                "--screen",
                "compare pixels where they are, for overlays fixed on screen",
            ),
            ("--grid <cols>x<rows>", "the cells; 16x9 by default"),
            (
                "--threshold <b>",
                "brightness change (0 to 1) a pixel needs to count; 0.04 by \
              default",
            ),
            ("--half", "work at half size, faster, misses thin lines"),
            IMAGE,
        ],
        output: &[
            ("reading", "one sentence, naming the strongest event"),
            ("pops, blips, flicker_runs", "how many of each"),
            (
                "events",
                "the 16 strongest: kind, from and to (frame, t, tick), box_px (the \
              cell: x, y, width, height in pixels from the top left), change (brightness 0 to \
              1 times pixels changed: a few units is a detail, tens is plain to see), near \
              (things whose centre is in or next to the cell)",
            ),
            (
                "changed_share",
                "the share of the picture that changed a frame: median, max",
            ),
            ("compensated", "true: the camera's motion was taken out"),
            (
                "warnings",
                "settings that may hide defects (a low frame rate, a small size)",
            ),
            (
                "image",
                "the last frame, dimmed, with where things changed coloured by when \
              (blue early, yellow late) and the events boxed with their times",
            ),
        ],
        writes: "<film>/motion/flicker-<options>.png",
        example: "galaxy-gm motion flicker films/note-0001",
        next: "galaxy-gm motion sheet <film> --at <from t> --crop <x,y,w,h of box_px> to \
               see the frames round an event.",
        topic: "motion",
    },
    Command {
        tool: Tool::Gm,
        path: "motion flow",
        synopsis: "<film> [--along course | --along x,y;x,y] [--screen]",
        answers: "Is something moving that has no name, or crawling where it should be still?",
        about: "Pyramidal Lucas-Kanade optical flow on a grid of textured points between \
                frames moved into each other's view. Points still in the world, or moving with \
                the parallax backdrop, are set aside; what moves steadily otherwise is \
                reported. Something anchored to the followed thing moves like the screen and \
                is set aside too: use strip or dots for that.",
        args: &[
            ("<film>", "a film folder with pictures"),
            (
                "--along course",
                "also measure the flow along the course leg",
            ),
            ("--along x,y;x,y", "or along this world path, world units"),
            ("--screen", "compare pixels where they are, for overlays"),
            (
                "--step <px>",
                "grid spacing of the points, 4 to 64; 12 by default",
            ),
            (
                "--region <x,y,w,h>",
                "only points in this box, pixels from the top left",
            ),
            IMAGE,
        ],
        output: &[
            ("reading", "one sentence"),
            (
                "moving",
                "cells moving steadily: box_px, px_per_frame [x, y], coherence (0 to \
              1, how steady the direction), share_of_frames, first (when), near_mid_film \
              (things near it)",
            ),
            (
                "along",
                "with --along: path, px_per_frame along it, frames_measured",
            ),
            (
                "backdrop_share_of_camera_step",
                "how much of the camera's step the backdrop \
              moves (parallax)",
            ),
        ],
        writes: "<film>/motion/flow-<options>.png",
        example: "galaxy-gm motion flow films/route --along course",
        next: "galaxy-gm motion sheet <film> --crop <x,y,w,h> round a moving cell.",
        topic: "motion",
    },
    Command {
        tool: Tool::Gm,
        path: "motion compare",
        synopsis: "<film> <other film> [--vdp]",
        answers: "What changed against an approved film: another build, or before a change?",
        about: "Scores each pair of frames with FLIP (colour differences filtered by what the \
                eye resolves, raised where edges differ). A frame differs visibly when 16 or \
                more pixels score 0.5 or more. A difference says where to look, not which film \
                is right. --vdp adds ColorVideoVDP, a model of seeing differences in video, in \
                JOD (10 is no visible difference); it needs `uv` on PATH and fetches PyTorch \
                on first use.",
        args: &[
            (
                "<film>, <other film>",
                "two films of the same moments at one size",
            ),
            ("--vdp", "also score with ColorVideoVDP"),
            ("--half", "compare at half size, faster"),
            (
                "--ppd <n>",
                "pixels per degree of view for FLIP; about 67 (a 4K screen at arm's \
              length) by default",
            ),
            IMAGE,
        ],
        output: &[
            ("reading", "one sentence"),
            (
                "flip.visibly_different_frames",
                "how many frames differ visibly",
            ),
            (
                "flip.worst_frames",
                "the five that differ most: frame, t, tick, mean (FLIP 0 \
              to 1), pixels_differing (scoring 0.5 or more), box_px round them",
            ),
            (
                "flip.mean, median_frame",
                "mean FLIP error over the films, and the median \
              frame's",
            ),
            (
                "same_world_frames",
                "frames whose worlds have the same hash",
            ),
            (
                "colorvideovdp",
                "with --vdp: the JOD score, or why it could not run",
            ),
            (
                "image",
                "the worst frame's error map beside a distogram (time down, across the \
              screen left to right)",
            ),
        ],
        writes: "<film>/motion/compare-<options>.png",
        example: "galaxy-gm motion compare films/after films/before",
        next: "galaxy-gm motion sheet on both films --at the worst frames' t.",
        topic: "motion",
    },
];

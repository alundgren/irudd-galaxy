# Audio

`galaxy-audio` owns the mixer. CPAL only opens the output device and supplies buffers;
hound decodes WAV and writes offline WAVs; lewton decodes Ogg Vorbis; symphonia's MP3
decoder (MPL-2.0, the only symphonia part built) decodes MP3. All are MIT-compatible.
There is no audio middleware and no per-frame Luau work.

## Module declarations

An optional `audio.toml` lives beside the manifest. Its sound names are local,
one-part names. Files are relative to the module, use `/`, and must not contain `..`.
WAV integer PCM (up to 32 bits), WAV 32-bit float, Ogg Vorbis and MP3 are supported,
mono or stereo. Ogg files contain one logical Vorbis stream, without chaining. The final
granule length trims codec padding so short loops keep their authored duration. MP3 is
trimmed by its LAME tag's encoder delay and padding when it has one; an untagged file
keeps a few milliseconds of codec silence at each end. MP3 frames that fail to decode are
skipped, as players do, and decoder overshoot past full scale is clamped. Generated
music arrives as MP3 (`docs/music-generation.md`).
Samples must be finite and within [-1, 1]. Mono feeds both stereo channels.
Clips are decoded into memory before playback; music should be kept reasonably small.

```toml
crossfade = 1.0                 # seconds for place changes, 0..60
[sounds.ping]
file = "sounds/ping.wav"
volume = 0.7                    # 0..1, default 1
[sounds.room]
file = "sounds/room.ogg"
volume = 0.3
[sounds.theme]
file = "sounds/theme.ogg"
volume = 0.3
loop_crossfade = 2.0            # seconds the loop's end fades into its start, 0..60

[places]
first = "room"                 # local or own full place ID -> local sound

[[events]]
event = "pressed"              # local or full event kind, e.g. engine.start
sound = "ping"
bus = "sound"                  # default: one-shot; music: looping
fade = 0.02                    # seconds, 0..60
[[events]]
event = "music_started"
sound = "theme"
bus = "music"
fade = 0.5
[[events]]
event = "music_stopped"
bus = "music"
stop = true                    # omit sound when stopping a bus
fade = 0.5
[[events]]
event = "beam_on"
sound = "hum"
channel = "beam"               # loop on this named channel of the sound bus
fade = 0.3
[[events]]
event = "beam_off"
channel = "beam"
stop = true                    # fades only that channel
fade = 0.3
[[events]]
event = "chip"
sound = "ping"
vary_pitch = 0.05              # rate 1 +/- 0.05, 0..0.5
vary_gain = 0.2                # gain 0.8..1, 0..1

[[follow]]                     # a channel's loop, or ambience = true
channel = "beam"
entity = "ship"                # local or full entity ID
component = "engine.motion"
field = ["vx", "vy"]           # one field, or a vector's length
# distance_to = "beacon"       # instead of component and field
from = 0.0
to = 14.0                      # from above to runs the curve backwards
gain = [0.4, 1.0]              # 0..1, times the cue's volume
pitch = [0.8, 1.6]             # 0.25..4
power = 1.0                    # curve shape, 0.1..10
ramp = 0.1                     # seconds each change glides, 0..10
```

`galaxy-gm fields audio` lists every key with its range and default, from the table the
parser is tested against.

Any new matching fact triggers a cue, including input facts and script-recorded facts.
Scripts can use the existing `ctx.record("pressed", {})`. Keyboard bindings produce
both down and up facts: a handler can record a separate sound event only on down, as
the test module does. Every event kind must already be declared in a loaded manifest
or by the engine. Multiple mappings may react to the same event, in module and table
order. A music cue crossfades from the previous music, even when retriggering the same
clip. Sound cues overlap. Stop fades all voices on that bus, channel loops included.

**Channel loops.** A sound cue with `channel` loops its sound on that channel of the sound
bus (so it is in the effects stem and follows the sound bus volume) until a `stop = true`
cue on the same channel fades it out. A held action therefore sounds from its start and
end facts, not from a repeating clip: a mining beam starts with the beam. The same sound
cued again on a playing channel keeps playing and only moves toward the new volume;
another sound crossfades over the cue's fade. Channel names are local to the module, and
all modules together have at most 32. Like music, a channel loop doesn't come back on its
own after a resume or a hot-load: the module records its start event again.

**Variation.** `vary_pitch` and `vary_gain` give each cue its own rate (1 ± `vary_pitch`)
and gain (1 - `vary_gain` up to 1, so never louder than declared). The draw is a hash of
the fact's tick, its payload's `entity`, the event kind and the cue's place in the table:
the same fact sounds the same in live play, `galaxy audio` and `audio mix`, on every OS,
and the simulation's random streams are never touched.

**Following a world value.** Each `[[follow]]` reads one number after every tick: a
field of a component on an entity, the length of a vector of fields, or the distance
between two entities' `engine.position`. Its place between `from` and `to` (clamped,
then raised to `power`) picks gain and rate linearly between the pairs. Several curves on
one loop multiply. The audio scene sends a change only when the result moved, and the
mixer glides there over `ramp` seconds, so a value scripts set once a second still sounds
smooth. `field = "vx"` reads the signed value; a list, even `["vx"]`, reads its length.
`check` rejects a component or field the loaded modules don't declare, or one that isn't
a number, since a typo would leave the loop silently at `from`. While the game runs, a
missing entity or a field with no value counts as `from`. Ambience follows apply to
whichever ambience is playing, through crossfades.

**Contacts.** The collide service records `engine.contact` (`entity`, `other`, `speed`)
when two bodies start touching (`flight-and-collision.md`). A module asks for it with
`contact_speed` in its manifest and cues a bump on it like any fact.

The marked `engine.being.player` selects an `engine.location.place`. That exact place
picks an ambience; parents are not searched. Places with no mapping, or worlds with
no player, are silent. A different ambience crossfades using the destination module's
`crossfade`; leaving for silence uses the last fade. Moving between places that use
the same clip leaves that loop playing. Place IDs may be created by scripts later;
the audio table does not create or alter places. Each module maps its own places.

`galaxy-gm check <module>` reports sound findings for bad tables, unknown cue sounds
or event kinds, missing files and decoding failures. It decodes every declared sound,
including unused sounds. The installed `galaxy-gm guide audio` describes this format.

## UI sounds

Screens play module sounds for what the player does to them, from a `[ui]` table:

```toml
[ui]
hover_gap = 0.08               # seconds at least between hover sounds, 0..1
hover = "tick"                 # the pointer enters a button, bubble, drag source or target
press = { sound = "click", vary_pitch = 0.05 }  # a click on a ready button, bubble, target or speaker
open = "open"                  # a panel, list or bubble appears
close = "close"                # ... or goes away
drag = "lift"                  # a drag source is picked up
drop = "place"                 # a held value lands on a target that takes it, or back home
deny = "thud"                  # a resting button is clicked, or a target refuses the value
[ui.kinds.drag_source]         # button, bubble, target, speaker, drag_source, panel, scroll
hover = { sound = "lift", vary_gain = 0.3 }
[ui.looks.chip]                # a look from [screen.looks] in style.toml
press = "coin"
```

Each action takes a sound name or `{ sound, vary_pitch, vary_gain }`, varied like an
event cue. A widget's sound is its look's, else its kind's, else the `[ui]` one; none of
them means silence. A drag's sounds (drag, drop, and deny on a refusing target) are its
source's. Like `[screen]`, the scene has one table: the last module by ID with `[ui]`
supplies it, so a sound module can dress every screen. Check reports unknown sounds,
keys, kinds and looks and out-of-range numbers.

**Where cues come from.** The view notices each action where it already decides it:
`click` (press, or deny on a resting button), pickup and release of a drag, a model
target's or speaker's click, the frame's pointer test that sets the hand cursor
(hover), and screen motion's record of surfaces coming and going (open, close). A panel,
list or bubble inside one that stays opens and closes on its own; inside one that comes
or goes it is part of that one's sound, so a panel arriving with a list in it opens once. A hover
fires once per thing entered; a widget with an `id` is the same thing when its screen
re-parents it. Sounds don't depend on motion keys or reduced motion. Each cue
(`galaxy_api::ui::UiCue`) carries the action, the widget's kind, look and path, and the
screen clock (tick plus interpolation), and waits in the screens until play takes it
after drawing that frame. Play sends its sound to the device at once, so a click is
heard within a frame plus the device buffer, and records the cue. The audio scene
drops a hover within `hover_gap` of the last hover sound, so sweeping a list ticks at
most that often; the cue itself is still recorded. Only a view that listens leaves cues:
check's staged scenes, notes, audit and film make none. The cues never reach the world.

**Offline.** A recording keeps every cue as a `ui_sound` event beside its presentation
snapshots. `galaxy-gm audio mix` of a scenario with `recording = "recording.json"`
(instead of `inputs`) runs the recording's world again and starts each cue's sound on
the mix frame its screen clock names, through the same scene, so the hover gap and
variation decide exactly as they did live. Its report lists `ui_sounds`: each cue's
time, action, kind, look, widget and the sound it played (null when silent). `galaxy
play`'s report counts cues by action, `played` and `silent`, so a run shows at a glance
which actions have no sound. `crates/gm/tests/ui_sounds.rs` drives every action through
a detached view, checks each cue, the gap and the world hash, and mixes the recording.

From the repository root (`engine/tests/play/ui-sounds`; sounds from its
`demo/generate.py`):

```sh
galaxy-gm check engine/tests/play/ui-sounds/demo
galaxy play --modules engine/tests/play/ui-sounds --inputs engine/tests/play/ui-sounds-inputs.toml --ticks 130
# mix.toml: modules = "<repo>/engine/tests/play/ui-sounds", recording = "<session>/recording.json"
galaxy-gm audio mix mix.toml --out mix.wav --stems stems
```

The bot hovers and presses the shelf button (the shelf opens), presses the chip (its
look's coin), the resting Locked (deny), drops the token on the narrow slot (deny) and
the wide one (drop), sweeps six rows a tick apart, and closes the shelf. That run's play
report counted 9 hovers (5 played, 4 thinned in the sweep), 3 presses, 2 drags, 2
denies, 1 drop, 1 open and 1 close; the mix of its recording heard all 19 cues at the
recorded times, and the sound stem rose from silence to -14 to -24 dBFS RMS within the
first 20 ms of every played cue, with no limited samples.

## The mixer

Clips hold immutable stereo frames shared by voices. A voice has its source frame
position, bus, gain ramp and stop state. Each output frame linearly interpolates the
two neighboring source frames, advances by source rate / output rate, and wraps music
and ambience. This lets the same bank play on devices with different sample rates.
One-shots end on their last frame.

**Loop seams.** A clip rarely ends where it starts: generated music ends on a decaying
note and starts on a full attack, so a plain wrap is a sudden step in level. Each sound
has a `loop_crossfade` (seconds, default 2). When music or ambience reaches its last
`loop_crossfade` seconds, the clip's start plays under it with a linear crossfade, and on
reaching the end the voice carries on from the point the start had reached. Each pass
after the first is therefore that much shorter than the file; the first pass still
starts at frame 0. A clip shorter than four fades wraps as authored: short loops are cut
to whole cycles, and starting the next pass early would put them out of phase. A long
loop cut to whole bars sets `loop_crossfade = 0` to keep its beat. The crossfade is a
second read of the same frames and a multiply, so the callback still allocates nothing.

A voice's rate is the clip's rate over the output rate, times its own cue variation and,
for channel loops and ambience, the rate its follow curve set. `Action::Tune` sets a gain
and rate ramp per channel (and one for the ambience) that every voice there multiplies
in; the ramps are a fixed array of 33 pairs, so tuning allocates nothing in the callback.
`Action::ResetTunes` puts them back to 1 when a hot-load brings a new bank, whose curves
tune again from its first update.

Ramps advance once per output sample, reaching their target in the requested number
of frames. New voices and stopped voices have at least a 5 ms edge ramp, and effects
fade over their final 5 ms. Crossfades are linear: old and new gains sum to one for
equal-volume clips. Rapid changes keep outgoing fades running to completion.

The master starts at 0.7 and the three bus gains at 1. `Action::Volume` adjusts a bus
or the master, with a ramp. Gains and decoded clip volume multiply. At most 32 voices
exist: a new voice steals the oldest effect first, otherwise the quietest loop. There
is no spatialization, priority language or virtual voice subsystem. Channel loops count as
loops for stealing. A final clamp
protects output [-1, 1]; offline `limited_samples` reveals mixes that hit it. Authors
should lower clip volumes until that count is zero rather than rely on the clamp.

The CPAL callback owns the mixer and consumes a bounded queue of prepared action
batches. It never takes a simulation lock, decodes files or calls scripts. Before
queueing a clip, the game thread retains one PCM reference per distinct buffer.
Every send, including empty ticks, reclaims buffers whose only remaining owner is
that retained reference. Queued actions and active voices therefore keep retired
banks' samples alive across hot-load fades; ending or stealing a voice cannot free
PCM in the callback. The stream and queue are dropped before these retained owners.
Frame mixing allocates nothing, and sample buffers are allocated and freed outside
the callback. The callback translates stereo floats to the device's
sample format, averages for mono, and silences channels above stereo. Missing devices,
unsupported output configurations, stream creation/start failures and stream errors
are notes, never simulation failures. A full queue drops cues with a note rather than
stall the game. Tests never open a device; the missing-device path is injected.

## Presentation and replay

Live play and offline rendering share a fact cursor and pass only newly appended
fact kinds and a read-only world view to the audio scene. A fresh session starts
the cursor at zero, so facts already present, including `engine.start`, play once
before the first tick alongside initial ambience. For a resumed world the cursor
starts after its saved facts; a hot-load starts just before its new facts. Neither
replays historical cues. Music that should still play after a resume comes back
through its module: the first tick of a resumed world starts with the input
`engine.resumed`, and a handler records the music event again, which the cursor plays
like any new fact (`saves-and-replay.md`, "Hearing a resume"). No mixer position, gain or voice is in the world, save or
hash. The session's existing event facts and place state are all that drive it. Pausing for an F8 note
leaves looping presentation audio running. `--watch` reloads banks outside the audio
callback, fades old voices out and selects the current ambience again; music starts
again on the next matching event.

Module text, including `audio.toml`, already travels with recordings. Binary sound
files remain external presentation assets in the module folders; headless replay
needs none of them and does not open a device. No world golden hash depends on audio.
`galaxy audio` renders ordinary scripted event inputs using those module folders; a
played session's recording, UI sounds included, mixes through `galaxy-gm audio mix`
(above).

## Offline proof and listening

From the repository root, using a build with audio (add `--build <name>` after the
binary name for a dev build):

```sh
galaxy-gm check engine/tests/play/audio/demo
galaxy play --modules engine/tests/play/audio
galaxy play --modules engine/tests/play/audio --inputs engine/tests/play/audio-inputs.toml --ticks 300
galaxy audio --modules engine/tests/play/audio --inputs engine/tests/play/audio-inputs.toml --ticks 300 --out mix.wav
```

Enter changes between two places: 220 Hz low ambience and 440 Hz high ambience,
crossfading over a second. Space plays a short 880 Hz ping. M starts a quiet chord
loop; S fades it out over half a second. Buttons do the same. The scripted run pings
at seconds 1 and 4, changes ambience at seconds 2 and 8, starts music at second 5,
and stops it at second 7. The first second fades the initial ambience in.

`engine/tests/play/loops/hum` shows the newer cues: W thrusts a ship into a fixed rock;
the 110 Hz engine hum loops on its own channel from the start with pitch 0.8 at rest and
1.6 at top speed, B holds a 330 Hz beam on a second channel, Space plays an 880 Hz ping
that varies ±10% in rate, the bump plays on `engine.contact`, and the bay's ambience
fades with the ship's distance from its start. `engine/tests/play/loops-inputs.toml` holds
the beam from 1 to 6 s, pings three times and thrusts at 7 s. `galaxy-gm audio mix` of
that run with `--stems` measured the beam's effects-stem level at -22.1 dBFS in every
half second from 1.0 to 6.0 s and gone after the 0.3 s fade, the hum at 88 Hz at rest and
176 Hz at speed, three pings at 1.066, 1.032 and 1.041 times 880 Hz at levels 2.5 dB
apart, one contact at the rock, and the ambience 10.7 dB down 17 units out (gain 0.29 of
0.25..1), with no limited samples. `crates/audio/tests/loops.rs` checks the same from the
rendered PCM and that the world's hash is the same with and without `audio.toml`. Its
sounds are made by `python engine/tests/play/loops/hum/generate.py`.

`galaxy audio` creates a new stereo 48 kHz 16-bit PCM WAV and reports peak, RMS,
limited samples, voice counts and world hash. It never opens a window or device.
Its options and limits are in `galaxy audio --help`.

The seam was measured on a real 30.8 s Lyria clip, rendered for 120 s at volume 1. With
`loop_crossfade = 0` the level 50 ms either side of each wrap goes from 0.032 to 0.0001
and then straight to full level (0.20 in the next 50 ms), which is the hard cut players
hear. With the default 2 s the largest sample step within 2 ms of each seam is 0.012,
1.7 times the clip's median step (0.0073) and a quarter of its largest step elsewhere
(0.052), and the quarter-second level runs 0.13, 0.10, 0.15, 0.18, 0.15, 0.17 across
the fade with no step. Tests check the same on a ramp, whose plain wrap jumps by 1. The small original fixture files
are reproduced by `python engine/tests/play/audio/demo/generate.py`; no downloaded music
is used. The Ogg decoder fixture is made from `sounds/high.wav` with
`oggenc -Q -s 7 -o sounds/high.ogg sounds/high.wav` in that module folder
(vorbis-tools). The MP3 decoder fixture `sounds/high.mp3` is the same WAV encoded at
32 kbit/s, quality 2, with the `lameenc` Python package (`Encoder()` with
`set_bit_rate(32)`, `set_in_sample_rate(16000)`, `set_channels(1)`, `set_quality(2)`;
`encode(pcm) + flush()`); it is untagged. On Linux, building CPAL needs `pkg-config` and the ALSA development
package (`libasound2-dev` on Ubuntu); playing without a device needs no special setup.
Tests measure frequency amplitudes across the fade, event and music windows,
exercise resampling, gain ramps, voice saturation, decoding failures and silent output,
and compare every scripted tick's world hash against the same module with its audio
table removed. Recording replay must reach the identical hash as well.

## Alternatives and why

A playback/middleware crate would own the system we want to learn, so CPAL is only
plumbing. Restoring music on resume by replaying the last music cue found in the saved
facts would need no module code, but a cue chosen from world state rather than from one
fact can't be read back; the `engine.resumed` event lets the module that chose the music
choose it again. A separate script sound command would require new presentation command and
replay plumbing; an event table reuses facts already present and works headless.
UI sounds as script-recorded facts would put every hover into the world and make sound
wait for the next tick; cues from the view play on the frame and stay out of hashes.
Deriving them later from presentation snapshots would miss a press and release within
one tick and a resting click, which leaves no snapshot change; recording each cue keeps
the offline mix exact. A table per kind and look in `audio.toml` keeps sounds beside the
other sounds; putting them on looks in `style.toml` would split one module's sounds
across two files and give unlooked widgets none.
A loop that restarts a short clip on each repeating fact relies on the fact's cadence
and starts with the first result rather than the action; a channel started and stopped
by facts sounds exactly as long as the action. Following values in Luau would mean
per-frame script work or once-a-second steps; reading a field in the audio scene after
each tick and gliding in the mixer costs nothing in the sim. Variation drawn from the
world's random streams would change world hashes whenever a sound is added; a hash of
the fact keeps sound presentation-only and still repeatable.
An ambience field on a world component would put presentation choices into saves and
hashes; a place-ID table keeps them in module presentation data. Predecoding is simple
and keeps I/O off the callback; streaming large music is unnecessary for this prototype.
MP3 is decoded rather than transcoded at generation time because a WAV of the same piece
is about ten times larger in git, and an Ogg encoder would be another native library.
Linear interpolation and crossfades are small and inspectable; higher-order resampling,
equal-power fades and a compressor are unnecessary for these authored clips. For loops,
an equal-power fade would keep uncorrelated material level through the seam (the linear
fade dips by up to 3 dB at its middle) but lifts a clip whose end matches its start by
3 dB; searching each clip for the best-matching loop point, or cutting at beats, would
need analysis the engine doesn't otherwise do. A fixed crossfade that shortens the loop
needs nothing beyond the frames already in memory.

## Authoring measurements

[Audio tools](audio-tools.md) describes `galaxy-gm audio inspect/process/loop/mix/judge/check`.
`Pcm::read` exposes decoded source channels and finite overshoots before playback
clamping, while `Clip::read` retains its existing playback behavior. `frame_with_buses`
returns the same final frame plus each post-gain bus and the pre-clamp sum, without
changing summation order or allocating in the callback. The offline `galaxy audio`
report adds `pre_clamp_peak` and per-bus `bus_rms`; the GM adds LUFS and overlapping
bus comparisons. Prepared loop files use `loop_crossfade=0` to avoid blending twice.

# Audio

`galaxy-audio` owns the mixer. CPAL only opens the output device and supplies buffers;
hound decodes WAV and writes offline WAVs; lewton decodes Ogg Vorbis. All three are
MIT-compatible. There is no audio middleware and no per-frame Luau work.

## Module declarations

An optional `audio.toml` lives beside the manifest. Its sound names are local,
one-part names. Files are relative to the module, use `/`, and must not contain `..`.
WAV integer PCM (up to 32 bits), WAV 32-bit float and Ogg Vorbis are supported, mono
or stereo. Ogg files contain one logical Vorbis stream, without chaining. The final
granule length trims codec padding so short loops keep their authored duration.
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
```

Any new matching fact triggers a cue, including input facts and script-recorded facts.
Scripts can use the existing `ctx.record("pressed", {})`. Keyboard bindings produce
both down and up facts: a handler can record a separate sound event only on down, as
the test module does. Every event kind must already be declared in a loaded manifest
or by the engine. Multiple mappings may react to the same event, in module and table
order. A music cue crossfades from the previous music, even when retriggering the same
clip. Sound cues overlap. Stop fades all voices on that bus.

The marked `engine.being.player` selects an `engine.location.place`. That exact place
picks an ambience; parents are not searched. Places with no mapping, or worlds with
no player, are silent. A different ambience crossfades using the destination module's
`crossfade`; leaving for silence uses the last fade. Moving between places that use
the same clip leaves that loop playing. Place IDs may be created by scripts later;
the audio table does not create or alter places. Each module maps its own places.

`galaxy-gm check <module>` reports sound findings for bad tables, unknown cue sounds
or event kinds, missing files and decoding failures. It decodes every declared sound,
including unused sounds. The installed `galaxy-gm guide audio` describes this format.

## The mixer

Clips hold immutable stereo frames shared by voices. A voice has its source frame
position, bus, gain ramp and stop state. Each output frame linearly interpolates the
two neighboring source frames, advances by source rate / output rate, and wraps music
and ambience. This lets the same bank play on devices with different sample rates.
One-shots end on their last frame. Authored loops should join cleanly at the seam.

Ramps advance once per output sample, reaching their target in the requested number
of frames. New voices and stopped voices have at least a 5 ms edge ramp, and effects
fade over their final 5 ms. Crossfades are linear: old and new gains sum to one for
equal-volume clips. Rapid changes keep outgoing fades running to completion.

The master starts at 0.7 and the three bus gains at 1. `Action::Volume` adjusts a bus
or the master, with a ramp. Gains and decoded clip volume multiply. At most 32 voices
exist: a new voice steals the oldest effect first, otherwise the quietest loop. There
is no spatialization, priority language or virtual voice subsystem. A final clamp
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
replays historical cues. No mixer position, gain or voice is in the world, save or
hash. The session's existing event facts and place state are all that drive it. Pausing for an F8 note
leaves looping presentation audio running. `--watch` reloads banks outside the audio
callback, fades old voices out and selects the current ambience again; music starts
again on the next matching event.

Module text, including `audio.toml`, already travels with recordings. Binary sound
files remain external presentation assets in the module folders; headless replay
needs none of them and does not open a device. This change adds no recording fields
and changes no existing world golden hash. `galaxy audio` renders ordinary scripted
event inputs using those module folders; it does not render a recording or UI clicks.

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

`galaxy audio` creates a new stereo 48 kHz 16-bit PCM WAV and reports peak, RMS,
limited samples, voice counts and world hash. It never opens a window or device.
Its options and limits are in `galaxy audio --help`. The small original fixture files
are reproduced by `python engine/tests/play/audio/demo/generate.py`; no downloaded music
is used. The Ogg decoder fixture is made from `sounds/high.wav` with
`oggenc -Q -s 7 -o sounds/high.ogg sounds/high.wav` in that module folder
(vorbis-tools). On Linux, building CPAL needs `pkg-config` and the ALSA development
package (`libasound2-dev` on Ubuntu); playing without a device needs no special setup.
Tests measure frequency amplitudes across the fade, event and music windows,
exercise resampling, gain ramps, voice saturation, decoding failures and silent output,
and compare every scripted tick's world hash against the same module with its audio
table removed. Recording replay must reach the identical hash as well.

## Alternatives and why

A playback/middleware crate would own the system we want to learn, so CPAL is only
plumbing. A separate script sound command would require new presentation command and
replay plumbing; an event table reuses facts already present and works headless.
An ambience field on a world component would put presentation choices into saves and
hashes; a place-ID table keeps them in module presentation data. Predecoding is simple
and keeps I/O off the callback; streaming large music is unnecessary for this prototype.
Linear interpolation and crossfades are small and inspectable; higher-order resampling,
equal-power fades and a compressor are unnecessary for these authored clips.

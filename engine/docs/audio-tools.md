# Audio authoring tools

`galaxy-gm audio` measures, repairs and judges prepared audio. These commands are
authoring work: no analysis or network call runs during play, and no measurement
touches simulation state, hashes or replay. Each command returns one JSON object
with `measurement_version: 1`, a `reading`, timestamped `findings` with numeric
evidence and `suggested_action`, and `detail`. Undefined numbers are `null`, with
reasons, never minus infinity or a silent zero. Help and `guide audio` ship with the GM.

## Inspect the source

```sh
galaxy-gm audio inspect clip.mp3 --target-lufs -23 --reference approved.wav
```

WAV PCM/float, Ogg Vorbis and MP3 reuse hound, lewton and Symphonia. `Pcm` preserves
unclamped finite samples and the original mono/stereo layout. Playback's `Clip`
still clamps MP3 overshoots and duplicates mono. Analysis accepts 8..192 kHz.
Only actual source channels count in LUFS; mono duplication adds 3.01 LU in playback.

Our BS.1770 meter applies the K-weighting shelf and RLB high pass separately per
channel. The coefficients are transformed for the sample rate, and tested against
the published 48 kHz values. It stores 100 ms energies, forms 400 ms momentary and
3 s short-term windows at 10 Hz, and computes integrated loudness with the absolute
-70 LUFS gate followed by a -10 LU relative energy gate. LRA uses short-term windows,
-70 LUFS/-20 LU gates and the 10th-to-95th percentile span. Incomplete windows are
not padded; short clips can have no integrated/short-term result. LRA on a short
clip is unstable even when defined. RMS and crest factor include silence.

True peak is a four-phase, 49-tap-per-phase windowed sinc estimate, including source
samples and zero-padded edges. It is not a certified compliance meter. Generated
tests validate a stereo 1 kHz sine at -23 dBFS amplitude against -23 LUFS, the mono
3.01 LU difference, 44.1/48/96 kHz agreement, relative gating, a ten-LU range and a
12 kHz signal whose intersample peak exceeds its samples. No large fixtures ship.

RustFFT supplies only FFT plumbing. A 2048-sample Hann window at 100 ms hops sums
channel powers (so opposite-polarity stereo cannot hide a bright band). Power-weighted
centroid, 95% roll-off and fractions in 200..500 Hz, 2..5 kHz and above 8 kHz describe
balance. Narrow peaks with over 12 dB prominence and over 1% of energy are listed.
These can be wanted musical partials. A band above Nyquist is undefined. Compare
fractions with an approved reference and compensate the reported loudness difference
when listening; there is no universal harshness threshold.

Findings include clipping/flat-top runs, isolated impulse residuals compared with
neighboring derivatives, per-second DC means and stereo correlation/imbalance/mono
fold-down loss, and head/tail blocks below -60 dBFS. An isolated impulse detector
cannot identify every musical click. Spectral flags are clues with actions, not
automatic rejection. The loop seam reports first/last 100 ms level difference and
instantaneous sample/slope jumps; the level difference catches a fade-out followed
by a loud restart even when the sample jump is tiny.

## Repair into a new file

```sh
galaxy-gm audio process clip.mp3 --out soft.wav --remove-dc --declick \
  --high-shelf 4000,-3 --target-lufs -23
galaxy-gm audio loop soft.wav --out bed.wav --blend 2 --search 1
```

Processing order is explicit: `--trim start,end`, `--trim-silence` (10 ms margin),
`--declick` (interpolate isolated samples), `--remove-dc` (channel means), low shelf,
high shelf, notch, linear fades, gain. Shelves take `Hz,dB`; notch takes `Hz,dB,Q`.
EQ gains are limited to gentle +/-6 dB, notch cuts only. Fade durations must fit.
Gain takes either `--target-lufs` or `--gain-db`. The default -1 dBTP ceiling can be
changed with `--peak`; gain is reduced if necessary and `gain.peak_limited` says so.
There is no hidden compressor: an unreachable target stays below target.

The output is float WAV, written through a temporary file and persisted without
replacement. Existing files, aliases and the source are refused. No metadata or
module table is edited. Before/after inspections use the same meter, and after
is measured from the decoded exported file.

Loop search compares possible head/tail overlaps using normalized stereo waveform
correlation and level similarity on a 20 ms endpoint grid, then refines endpoints
within 1 ms for sample/slope compatibility. `--search 0` keeps the endpoints.
It overlaps tail into head and rotates past the consumed head; the output is shorter
by the overlap and any endpoint trimming. `--curve linear` suits correlated material;
`equal-power` can reduce dips for uncorrelated material but boost correlated material.
Gain/peak constraints run after blending. The report checks the edit boundary and
three decoded repetitions, not just the original endpoint. Set `loop_crossfade=0`
in `audio.toml` for a prepared loop to avoid an extra playback crossfade. Listen
across both boundaries; numeric continuity does not prove harmony or rhythm.

## Measure the real mix

```toml
# mix.toml; paths relative to this file
modules = "modules"
ticks = 1800
seed = 1
# inputs = "route.toml"
```

```sh
galaxy-gm audio mix mix.toml --out mix.wav --stems stems
```

The same `Scene` and `Mixer` as play and `galaxy audio` run an ordinary scripted
session, with event inputs and no window/device. Clicks, pointer gestures and hot
loads are refused. Up to one hour can be rendered. Source loudness is measured
before gains and playback clamp. Each bus is captured in the actual mixer pass
after voice, bus and master ramps; crossfades and voice stealing therefore affect
its effective level exactly as in play. The original voice summation order is
preserved for final playback. Pre-clamp and final levels are separate. The existing
`galaxy audio` renderer also reports `bus_rms` and `pre_clamp_peak`.

Optional stems are float WAV before the shared clamp, so sums can exceed full scale.
The report compares simultaneous one-second RMS levels and overlap between normalized
spectral bands. Similar spectra at similar levels suggest masking, not proof of it.
The raw source's nominal volume/master gain is informative; bus measurements are
authoritative. No universal background LUFS target exists. A starting experiment
is music's rendered contribution near -26 LUFS, tuned against effects/ambience.
Short effects need peak and momentary measurements. Digital levels cannot measure
the listener's physical sound pressure.

## Judge intent explicitly

```sh
galaxy-gm audio judge --intent "quiet warm background pads" bed.wav
```

The default setting is `google/gemini-3.8-flash`, verified with OpenRouter's model
catalog on 2026-11-03; `--model vendor/model` overrides it. The key comes only from
`OPENROUTER_API_KEY`. The shared client redacts the key and long fragments from
success, error, metadata and reflected prompts; model IDs containing it are refused.
Tests inject canned HTTP replies and never use a network or a real key.

There are exactly two non-streamed requests, no retries. First, fresh PCM WAV with
no filename/tags and no intent asks for a neutral description with timestamped mood,
instruments, energy, vocals and certainty. Only after a valid response does the
second request supply intent, the same audio and the blind description. It requires
fit/mismatch/uncertain and timestamped findings for mood, instruments, energy and fit,
each with an action and certainty. Invalid/missing timestamps or fields fail clearly.
Clips over 120 seconds are refused before any call; trim a representative excerpt.
Each call reports the actual model, generation ID, usage and provider-reported cost.
Missing actual cost remains null. A failed second call still reports the first call's
usage/cost. Semantic opinions are fallible and do not override numeric defects.

## Numeric acceptance

```toml
clip = "bed.wav" # or scenario = "mix.toml", exactly one
[[expect]]
value = "loudness.integrated_lufs"
at_least = -24.0
at_most = -22.0
[[expect]]
value = "seam.level_jump_db"
at_most = 3.0
[[expect]]
value = "defects.clicks"
equals = 0
```

`galaxy-gm audio check spec.toml` runs the local analysis once and returns `got`,
bounds, `pass` and suggested actions. Paths are relative to the spec. Dotted report
paths can index arrays numerically. A mix spec can use `buses.music.integrated_lufs`,
`pre_clamp.sample_peak_dbfs` or `limited_samples`. Undefined values fail, unknown
fields are bad input, and a failed numeric expectation exits 5 `audio_expectations`.
The check never calls a model. Keep targets appropriate to the authored intent.

## When processing cannot fix it

| Finding | Repair or prompt change |
|---|---|
| Too loud | Numeric gain; describe a restrained background bed, then verify playback. |
| Damaged flat tops | Attenuation prevents more clipping but cannot restore data; regenerate clean, undistorted, restrained dynamics. |
| Clicks | Local interpolation or tiny fades; smooth sustained transitions, though prompting is unreliable here. |
| DC / silence | Remove DC or trim with margins; immediate continuous texture, no silent intro/outro. |
| Sharp / muddy | Gentle shelf/notch with reference; rounded attacks, muted timbres, soft upper register / sparse voicing, clear low mids. |
| Stereo cancellation | Regenerate stable centred bass, mono-compatible stereo; gain cannot recover cancellation. |
| Abrupt loop / wrong content | Blend and listen; continuous repeating phrase, no final cadence. Change mood/instrument wording when content is wrong. |

## Alternatives and why

The meter and repairs are local Rust to keep the learning goal and reproducible,
headless operation. RustFFT is MIT/Apache plumbing; there is no AGPL dependency or
FFmpeg. A full mastering library would hide the systems we want to learn. Beat/chroma
ranking, CLAP and aesthetics were optional research extensions; they are not required
for waveform loop preparation. Model judgments complement measurement because neither
LUFS nor spectrum can establish musical intent. Two requests enforce blind description;
asking for both in one prompt would reveal intent too early.

References: [ITU-R BS.1770](https://www.itu.int/rec/R-REC-BS.1770),
[EBU Tech 3342](https://tech.ebu.ch/docs/tech/tech3342.pdf),
[OpenRouter audio inputs](https://openrouter.ai/docs/guides/overview/multimodal/audio).

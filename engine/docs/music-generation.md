# Music generation

`galaxy-gm music` makes music for a module at build time. One run asks a music model
through OpenRouter for one piece, writes it into the module as an audio file and declares
it in the module's `audio.toml`, where the sound system (`docs/audio.md`) plays it like
any authored sound. The game never calls a model: `galaxy` has no HTTP client, and music
exists only as files a module ships.

```sh
galaxy-gm music <module> --name calm --prompt "slow warm pads, quiet and unhurried" \
  --event engine.start --volume 0.4
galaxy-gm check <module>
galaxy play --modules <modules folder>
```

## How it works

The command judges everything it can before spending anything: the options, the module,
the sound name (a one-part local name), the cue's event (declared by a loaded manifest or
the engine) and whether it would overwrite a sound in `audio.toml` or a file at
`music/<name>.mp3|wav|ogg` (only with `--replace`). Only then does it read the key, and
only then make the call.

**The key.** `OPENROUTER_API_KEY` from the environment, nowhere else. Unset or blank is
exit 2, kind `no_key`, and nothing is written. The key is only ever placed in the
`Authorization` header. Its `Debug` is redacted, and every text that leaves the client
(an error, a body excerpt, the model's text, the prompt, the shape of a failed answer)
first goes through a redactor that removes the key or any run of 12 or more of its
characters however JSON spells them (plain, or `\uXXXX` escapes behind any number of
backslashes). Redaction runs before any truncation, so a cut can't leave a long piece. The
client redacts every text it returns, the answer's own `id` and `model` included, and the
command redacts every string of what it reports (JSON on success, the error and its fields
on failure) and the `audio.toml` it writes, so an option or answer field echoing the key
can't carry it out. A `--model` holding the key is refused as a usage error before the
call, since it would otherwise be sent as the model. There are no spend caps or budgets in
the command: the key carries its own limit.

**The call.** One `POST /api/v1/chat/completions` with `modalities: ["text", "audio"]` and
`stream: true` (OpenRouter streams audio output). The prompt gets "Instrumental only: no
vocals, no singing, no lyrics." appended. The default model is
`google/lyria-3-clip-preview` (about 30 seconds of 44.1 kHz stereo MP3, around 750 KB;
listed at $0.04); `--model` takes any OpenRouter model with audio output, such as
`google/lyria-3-pro-preview` for a longer piece. A paid call is never retried behind the
user's back: one run, one call.

**The answer.** The stream (comment lines while the model works, then `data:` chunks) is
folded into one body: deltas' `content`, `audio.data` and `audio.transcript` are joined in
order, and `id`, `model` and `usage` come from the chunks that carry them. The audio is
base64 at `choices[0].message.audio.data` (or an audio array or content part, or a
`data:` URL); the decoder accepts chunks padded one by one and joined. The file's own
first bytes decide its format (ID3 or an MPEG frame sync is MP3, `RIFF....WAVE` WAV,
`OggS` Ogg); anything else is rejected. Lyria answers MP3 whatever format is asked for.

**Writing.** The bytes go to `music/<name>-partial.<ext>` beside their destination, are
decoded with the sound system's own decoder, and only if that succeeds are moved to
`music/<name>.<ext>`. `audio.toml` is edited with `toml_edit`, so the author's comments,
order and other entries stay: `[sounds.<name>]` gets `file` and `volume` (0.5 by default,
or the volume it had when replacing) behind a comment naming the model that answered and
the prompt. When replacing, the author's comments above the table and beside its values
stay; only an earlier generation note is swapped for the new one. `--event` adds one
`[[events]]` cue on the music bus (looping, crossfading from other music) unless the same
cue exists. The edited file is written beside and moved over. The command then loads the
module's sound bank again and reports its findings as `check` would.

**Failures.** A refused call (HTTP error, or an error object inside a 2xx stream; 401 a
bad key, 402 no credit), no answer at all, an answer without audio or audio that doesn't
decode is exit 1, kind `openrouter`, with `status`, `model` and `written: false`; an
answer without audio also carries its shape (long strings shortened to a length note),
its `generation_id` and the `cost_usd` OpenRouter reported, so a surprise can be looked
into without another call. Nothing is written in any of these cases.

**Tests** never touch the network or need a key. The HTTP layer is a one-method trait;
tests answer with streams shaped like a recorded Lyria answer, carrying a small original
MP3 (`tests/play/audio/demo/sounds/high.mp3`). They cover folding, redaction of every
text path, refusals, unplayable audio, the `audio.toml` edit, `--replace` and the no-key
path through the real binary.

## Trying it

`tests/play/music/quiet` is a generic module with one place and no sounds. Copy it
somewhere outside the repository, run `galaxy-gm music` on the copy with `--event
engine.start`, then `galaxy play --modules <the copy's parent folder>`: the music starts
with the first frame and loops. `galaxy audio --modules <folder> --ticks 300 --out mix.wav`
renders the mix without a device.

## Alternatives and why

- **Generating while playing.** Ruled out for now: the game makes no model calls, so play
  works offline, costs nothing and replays the same. Music is a module asset like a model.
- **A spend cap and a cache index (the slice's design).** The slice generated during
  play, so it needed caps, bookings and a per-save cache. At build time a person or agent
  runs the command on purpose and the key has its own limit; the committed file is the
  cache.
- **Retries.** A 5xx or a lost connection may still bill, and a retry loop would spend
  without being asked. One call per run keeps the cost what the user asked for.
- **Transcoding to WAV or Ogg.** WAV is about ten times the size of the MP3 for the same
  piece, which matters with plain git; Ogg would need an encoder. The sound system
  decodes MP3 instead (symphonia's MP3 decoder, plumbing like hound and lewton).
- **A full OpenRouter SDK.** One POST, a stream fold and a parser are all this needs, and
  small code is easier to keep the key out of.

"""Reproduce the tiny PCM fixtures; Python standard library only.

Loops are whole cycles of whole-hertz tones over exactly one second, so they repeat
seamlessly (audio.toml sets loop_crossfade = 0)."""
import math
import pathlib
import struct
import wave

RATE = 16000
ROOT = pathlib.Path(__file__).parent / "sounds"
ROOT.mkdir(exist_ok=True)


def write(name, samples):
    with wave.open(str(ROOT / (name + ".wav")), "wb") as out:
        out.setparams((1, 2, RATE, len(samples), "NONE", "not compressed"))
        out.writeframes(struct.pack("<" + "h" * len(samples), *samples))


def tone(seconds, frequencies, level, envelope=None):
    count = round(RATE * seconds)
    samples = []
    for n in range(count):
        value = sum(math.sin(2 * math.pi * hz * n / RATE) for hz in frequencies)
        value *= level / len(frequencies)
        if envelope:
            value *= envelope(n, count)
        samples.append(round(value * 32767))
    return samples


write("hum", tone(1.0, [110, 220], 0.3))
write("beam", tone(1.0, [330], 0.25))
write("room", tone(1.0, [165], 0.2))
write("ping", tone(0.18, [880], 0.25, lambda n, c: min(n / 80, (c - n) / 200, 1)))
write("bump", tone(0.3, [140], 0.4, lambda n, c: min(n / 40, 1) * (1 - n / c) ** 2))

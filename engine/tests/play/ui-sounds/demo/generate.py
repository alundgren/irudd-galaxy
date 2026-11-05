"""Reproduce the UI sound fixtures: short tones, one pitch each; standard library only."""
import math
import pathlib
import struct
import wave

RATE = 16000
ROOT = pathlib.Path(__file__).parent / "sounds"
ROOT.mkdir(exist_ok=True)
for name, seconds, hz in [
    ("tick", 0.03, 2400),
    ("click", 0.05, 1200),
    ("coin", 0.08, 1800),
    ("open", 0.12, 660),
    ("close", 0.12, 520),
    ("lift", 0.06, 900),
    ("place", 0.08, 400),
    ("thud", 0.12, 160),
]:
    count = round(RATE * seconds)
    samples = []
    for n in range(count):
        value = 0.3 * math.sin(2 * math.pi * hz * n / RATE)
        value *= min(n / 40, (count - n) / 120, 1)
        samples.append(round(value * 32767))
    with wave.open(str(ROOT / (name + ".wav")), "wb") as out:
        out.setparams((1, 2, RATE, count, "NONE", "not compressed"))
        out.writeframes(struct.pack("<" + "h" * count, *samples))

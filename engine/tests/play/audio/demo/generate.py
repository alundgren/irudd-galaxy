"""Reproduce the tiny original PCM fixtures; Python standard library only."""
import math
import pathlib
import struct
import wave

RATE = 16000
ROOT = pathlib.Path(__file__).parent / "sounds"
ROOT.mkdir(exist_ok=True)
for name, seconds, frequencies in [
    ("low", 0.1, [220]),
    ("high", 0.1, [440]),
    ("ping", 0.18, [880]),
    ("music", 0.5, [330, 550, 660]),
]:
    count = round(RATE * seconds)
    samples = []
    for n in range(count):
        value = sum(math.sin(2 * math.pi * hz * n / RATE) for hz in frequencies)
        value *= 0.25 / len(frequencies)
        if name == "ping":
            value *= min(n / 80, (count - n) / 200, 1)
        samples.append(round(value * 32767))
    with wave.open(str(ROOT / (name + ".wav")), "wb") as out:
        out.setparams((1, 2, RATE, count, "NONE", "not compressed"))
        out.writeframes(struct.pack("<" + "h" * count, *samples))

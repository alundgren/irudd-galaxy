"""Synthesises the core's short sound effects as raw mono 48 kHz clips.

    python make.py <out-folder>

Needs numpy and scipy. It writes raw 32-bit float clips into <out-folder>.

Every clip is built from sines, decaying modes and filtered noise with a fixed seed, so
the same run gives the same files. They are deliberately soft: rounded attacks, little
above 4 kHz, low and mid tones. The raw clips are then levelled and measured with
`galaxy-gm audio process` and `galaxy-gm audio inspect` (see audio.toml); the files in
this folder are those processed results. Every clip gets `--remove-dc --peak -1`, and:

    mine_hum                                  --target-lufs -18
    mine_chip1-3, panel_open/close, talk_blip, ui_click
                                              --gain-db 3 --trim-silence
    part_drop                                 --gain-db 5 --trim-silence
    every other effect                        --target-lufs -18 --trim-silence

mine_hum is one second of sound that repeats exactly every second (whole-hertz partials
and a one-second periodic noise band), with 0.12 s linear fades at both ends: mining
plays it on every `mined` fact, once a second, so each copy fades into the next with
the same phase and the hum sounds continuous for as long as mining goes on. The three
chips take turns (scripts/sound.luau), so a second's chip is not the last one again.
"""

import sys
from pathlib import Path

import numpy as np
from scipy import signal
from scipy.io import wavfile

RATE = 48000
rng = np.random.default_rng(1234)


def t_axis(seconds):
    return np.arange(int(round(seconds * RATE))) / RATE


def decay(t, tau, attack=0.003):
    """An exponential decay with a short raised-cosine attack."""
    env = np.exp(-t / tau)
    a = int(attack * RATE)
    if a > 1:
        env[:a] *= 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, a))
    return env


def swell(t, attack, tau):
    """Rises over `attack` seconds, then decays."""
    env = np.where(t < attack, np.sin(0.5 * np.pi * np.clip(t / attack, 0, 1)) ** 2, np.exp(-(t - attack) / tau))
    return env


def noise(n):
    return rng.standard_normal(n)


def band(x, lo, hi, order=2):
    sos = signal.butter(order, [lo, hi], btype="bandpass", fs=RATE, output="sos")
    return signal.sosfilt(sos, x)


def low(x, hz, order=2):
    sos = signal.butter(order, hz, btype="lowpass", fs=RATE, output="sos")
    return signal.sosfilt(sos, x)


def glide(t, f0, f1, seconds):
    """Phase of a sine whose frequency moves from f0 to f1 over `seconds`, then holds."""
    k = np.clip(t / seconds, 0, 1)
    f = f0 + (f1 - f0) * (k * k * (3 - 2 * k))
    return 2 * np.pi * np.cumsum(f) / RATE


def tone(t, f, tau, partials=((1, 1.0),), attack=0.004):
    out = np.zeros_like(t)
    for mult, amp in partials:
        out += amp * np.sin(2 * np.pi * f * mult * t) * decay(t, tau / (1 + 0.6 * (mult - 1)), attack)
    return out


def place(out, clip, at):
    i = int(round(at * RATE))
    end = min(len(out), i + len(clip))
    out[i:end] += clip[: end - i]
    return out


def finish(x, fade_out=0.02, cutoff=6000):
    x = low(x, cutoff, order=2)
    x = x - np.mean(x)
    n = int(fade_out * RATE)
    x[-n:] *= np.linspace(1, 0, n)
    x[: int(0.001 * RATE)] *= np.linspace(0, 1, int(0.001 * RATE))
    return x / (np.max(np.abs(x)) + 1e-12) * 0.5


# Flying -------------------------------------------------------------------------------

def thrust_on():
    t = t_axis(0.7)
    raw = noise(len(t))
    n = (band(raw, 150, 700) + 0.4 * band(raw, 600, 1500)) * swell(t, 0.10, 0.22)
    return finish(n)


def thrust_off():
    t = t_axis(0.6)
    raw = noise(len(t))
    k = np.clip(t / 0.35, 0, 1)
    n = (band(raw, 150, 800) * (1 - k) + band(raw, 120, 320) * k + 0.35 * band(raw, 600, 1700) * (1 - k)) * swell(t, 0.02, 0.14)
    return finish(n)


def boost():
    t = t_axis(1.0)
    raw = noise(len(t))
    k = np.clip(t / 0.3, 0, 1)
    n = (band(raw, 200, 600) * (1 - k) + band(raw, 250, 1500) * k) * swell(t, 0.16, 0.28)
    return finish(n)


# Mining -------------------------------------------------------------------------------

def mine_hum():
    t = t_axis(1.12)
    hum = np.zeros_like(t)
    for f, a in ((110, 0.1), (220, 0.5), (330, 0.16), (440, 0.22), (660, 0.08), (880, 0.05), (1320, 0.02)):
        hum += a * np.sin(2 * np.pi * f * t)
    hum *= 1 + 0.05 * np.sin(2 * np.pi * 1 * t)
    # A band of noise that repeats exactly every second: random phases on whole-hertz bins.
    spec = np.zeros(RATE // 2 + 1, dtype=complex)
    bins = np.arange(len(spec))
    keep = (bins >= 600) & (bins <= 2000)
    spec[keep] = np.exp(2j * np.pi * rng.random(keep.sum())) / np.sqrt(bins[keep] / 300)
    second = np.fft.irfft(spec, n=RATE)
    second /= np.max(np.abs(second))
    hiss = np.tile(second, 2)[: len(t)] * 0.3
    x = hum + hiss
    x = x / np.max(np.abs(x)) * 0.5
    f = int(0.12 * RATE)
    x[:f] *= np.linspace(0, 1, f)
    x[-f:] *= np.linspace(1, 0, f)
    return x  # no filtering: it would break the exact one-second period


def chip(grains, lo, hi, ping):
    """A light crumble: a few grains of grit and a faint ping. Three variants take turns."""
    t = t_axis(0.3)
    x = tone(t, ping, 0.012, ((1, 1.0), (1.7, 0.3)), 0.003) * 0.18
    for at, a, tau in grains:
        place(x, band(noise(len(t)), lo, hi) * decay(t, tau, 0.002) * a, at)
    return finish(x, cutoff=3000)


def mine_chip1():
    return chip(((0.0, 1.0, 0.008), (0.028, 0.6, 0.006), (0.065, 0.4, 0.007)), 700, 2200, 940)


def mine_chip2():
    return chip(((0.0, 0.8, 0.007), (0.018, 1.0, 0.008), (0.05, 0.35, 0.006), (0.09, 0.25, 0.006)), 600, 1900, 820)


def mine_chip3():
    return chip(((0.0, 1.0, 0.009), (0.045, 0.5, 0.007)), 800, 2400, 1060)


def hold_full():
    t = t_axis(1.6)
    bell = ((1, 1.0), (2.0, 0.18), (3.0, 0.05))
    x = np.zeros_like(t)
    place(x, tone(t, 523.25, 0.45, bell), 0.0)
    place(x, tone(t, 783.99, 0.5, bell) * 0.85, 0.14)
    return finish(x, fade_out=0.1, cutoff=5000)


# Docking ------------------------------------------------------------------------------

def thump(t, f0, f1, tau, lp=500, grit=0.6):
    return np.sin(glide(t, f0, f1, 0.08)) * decay(t, tau, 0.004) + low(noise(len(t)), lp) * decay(t, tau / 3, 0.002) * grit


def dock_clunk():
    t = t_axis(0.9)
    x = thump(t, 110, 88, 0.09, lp=700) * 0.8
    for f, a, tau in ((196, 0.35, 0.15), (294, 0.25, 0.11), (440, 0.12, 0.07)):
        x += a * np.sin(2 * np.pi * f * t) * decay(t, tau)
    latch = tone(t, 880, 0.015, ((1, 0.5), (0.48, 0.6)), 0.001) + band(noise(len(t)), 600, 1800) * decay(t, 0.008, 0.001) * 0.4
    place(x, latch * 0.5, 0.17)
    return finish(x, fade_out=0.1, cutoff=4000)


def undock_clunk():
    t = t_axis(0.9)
    x = np.zeros_like(t)
    latch = tone(t, 880, 0.015, ((1, 0.5), (0.48, 0.6)), 0.001) + band(noise(len(t)), 600, 1800) * decay(t, 0.008, 0.001) * 0.4
    place(x, latch * 0.55, 0.0)
    place(x, thump(t, 120, 96, 0.08, lp=700, grit=0.4) * 0.7, 0.05)
    place(x, band(noise(len(t)), 250, 1300) * swell(t, 0.05, 0.22) * 0.35, 0.08)
    return finish(x, fade_out=0.1, cutoff=4000)


# Screens ------------------------------------------------------------------------------

def ui_click():
    t = t_axis(0.09)
    x = np.sin(2 * np.pi * 1150 * t) * decay(t, 0.006, 0.0008) + 0.7 * np.sin(2 * np.pi * 580 * t) * decay(t, 0.013, 0.0008)
    return finish(x, fade_out=0.02, cutoff=5000)


def blips(notes):
    t = t_axis(0.34)
    x = np.zeros_like(t)
    for at, f in notes:
        place(x, tone(t, f, 0.06, ((1, 1.0), (2.0, 0.15)), 0.004), at)
    swish = band(noise(len(t)), 900, 2600) * swell(t, 0.05, 0.08) * 0.12
    return finish(x + swish, fade_out=0.05, cutoff=5000)


def panel_open():
    return blips(((0.0, 523.25), (0.07, 783.99)))


def panel_close():
    return blips(((0.0, 783.99), (0.07, 523.25)))


def ding(t, f=783.99, tau=0.35):
    return tone(t, f, tau, ((1, 1.0), (2.0, 0.12), (3.0, 0.03)), 0.003)


def receipt():
    t = t_axis(0.9)
    stamp = np.sin(2 * np.pi * 180 * t) * decay(t, 0.03, 0.002) + low(noise(len(t)), 1200) * decay(t, 0.01, 0.001) * 0.5
    x = stamp * 0.7
    place(x, ding(t) * 0.8, 0.06)
    return finish(x, fade_out=0.1, cutoff=5000)


def coin(t, f):
    return tone(t, f, 0.07, ((1, 1.0), (1.5, 0.45), (2.02, 0.25)), 0.001)


def sale_coin():
    t = t_axis(1.0)
    x = np.zeros_like(t)
    for at, f, a in ((0.0, 1480, 0.7), (0.065, 1320, 0.55), (0.14, 1560, 0.45)):
        place(x, coin(t, f) * a, at)
    place(x, ding(t, 1046.5, 0.32) * 0.6, 0.22)
    return finish(x, fade_out=0.12, cutoff=4500)


def buy_goods():
    t = t_axis(0.85)
    x = thump(t, 120, 92, 0.07, lp=700, grit=0.5) * 0.8
    x += 0.25 * np.sin(2 * np.pi * 262 * t) * decay(t, 0.05)
    place(x, coin(t, 1320) * 0.35, 0.09)
    place(x, ding(t, 880.0, 0.3) * 0.6, 0.17)
    return finish(x, fade_out=0.1, cutoff=4500)


# Fitting ------------------------------------------------------------------------------

def part_drop():
    t = t_axis(0.5)
    x = thump(t, 140, 120, 0.06, lp=900, grit=0.5)
    for f, a, tau in ((310, 0.4, 0.06), (520, 0.25, 0.045), (780, 0.12, 0.03)):
        x += a * np.sin(2 * np.pi * f * t) * decay(t, tau, 0.002)
    lock = tone(t, 640, 0.02, ((1, 1.0), (1.6, 0.3)), 0.001) * 0.35
    place(x, lock, 0.09)
    return finish(x, fade_out=0.08, cutoff=4000)


def refused():
    t = t_axis(0.45)
    x = np.zeros_like(t)
    for at, f in ((0.0, 329.63), (0.11, 261.63)):
        place(x, tone(t, f, 0.07, ((1, 1.0), (2.0, 0.2)), 0.004) + low(noise(len(t)), 400) * decay(t, 0.02, 0.002) * 0.3, at)
    return finish(x, fade_out=0.08, cutoff=3000)


# Talk and radio -----------------------------------------------------------------------

def talk_blip():
    t = t_axis(0.13)
    ph = glide(t, 520, 760, 0.04)
    x = (np.sin(ph) + 0.1 * np.sin(2 * ph)) * decay(t, 0.035, 0.005)
    return finish(x, fade_out=0.03, cutoff=4000)


def radio_tune():
    t = t_axis(0.65)
    raw = band(noise(len(t)), 400, 2800)
    crackle = 0.6 + 0.4 * low(np.abs(noise(len(t))), 40)
    hiss = raw * crackle * swell(t, 0.04, 0.16)
    whistle = np.sin(glide(t, 940, 620, 0.35)) * swell(t, 0.06, 0.14) * 0.18
    return finish(hiss + whistle, fade_out=0.12, cutoff=4000)


def built():
    t = t_axis(1.8)
    x = np.zeros_like(t)
    bell = ((1, 1.0), (2.0, 0.15), (3.0, 0.04))
    for at, f, a in ((0.0, 523.25, 0.8), (0.13, 659.26, 0.8), (0.26, 783.99, 0.85), (0.42, 1046.5, 0.6)):
        place(x, tone(t, f, 0.5, bell) * a, at)
    return finish(x, fade_out=0.15, cutoff=5000)


SOUNDS = {
    "thrust_on": thrust_on, "thrust_off": thrust_off, "boost": boost,
    "mine_hum": mine_hum, "mine_chip1": mine_chip1, "mine_chip2": mine_chip2, "mine_chip3": mine_chip3, "hold_full": hold_full,
    "dock_clunk": dock_clunk, "undock_clunk": undock_clunk,
    "ui_click": ui_click, "panel_open": panel_open, "panel_close": panel_close,
    "receipt": receipt, "sale_coin": sale_coin, "buy_goods": buy_goods,
    "part_drop": part_drop, "refused": refused,
    "talk_blip": talk_blip, "radio_tune": radio_tune, "built": built,
}


def write(path, x):
    """32-bit float, so no rounding flattens a peak before `audio process` levels it."""
    wavfile.write(str(path), RATE, x.astype(np.float32))


if __name__ == "__main__":
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for name, make in SOUNDS.items():
        write(out / f"{name}.wav", make())
        print(name)

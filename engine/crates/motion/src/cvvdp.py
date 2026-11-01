# /// script
# requires-python = ">=3.10"
# dependencies = ["cvvdp", "numpy", "imageio", "matplotlib"]
# ///
"""ColorVideoVDP on two films, for `galaxy-gm motion compare --vdp`.

Usage: cvvdp.py <reference frames> <test frames> <fps> <half 0|1> <out.json> <distogram.png>

Reads the PNG frames of both films, scores the test film against the reference as video
(a JOD score: 10 is no visible difference, lower is more), writes the distogram and a
JSON summary with the frames where the difference is largest. galaxy-gm writes this file
and runs it with `uv run --script`; it is not meant to be run by hand.
"""

import json
import pathlib
import sys

import imageio.v3 as iio
import numpy as np
import pycvvdp
import torch


def frames(folder, half):
    files = sorted(pathlib.Path(folder).glob("*.png"))
    out = []
    for f in files:
        img = iio.imread(f)[:, :, :3]
        if half:
            h, w = img.shape[0] // 2 * 2, img.shape[1] // 2 * 2
            img = img[:h, :w].reshape(h // 2, 2, w // 2, 2, 3).mean(axis=(1, 3)).astype(np.uint8)
        out.append(img)
    return np.stack(out)


def main():
    ref_dir, test_dir, fps, half, out_json, distogram = sys.argv[1:7]
    half = half == "1"
    ref = frames(ref_dir, half)
    test = frames(test_dir, half)
    n = min(len(ref), len(test))
    ref, test = ref[:n], test[:n]
    device = torch.device("cuda") if torch.cuda.is_available() else torch.device("cpu")
    metric = pycvvdp.cvvdp(display_name="standard_fhd", device=device, quiet=True)
    jod, stats = metric.predict(test, ref, dim_order="FHWC", frames_per_second=float(fps))
    # Per frame: the largest drop below 10 JOD over channels and bands.
    q = torch.as_tensor(stats["Q_per_ch"], device=metric.device)
    ch_no = q.shape[1]
    q[:, :, :, -1] *= metric.baseband_weight[0:ch_no].view(-1, 1)
    q *= metric.get_ch_weights(ch_no) * ch_no
    drop = (10.0 - metric.met2jod(q)).cpu().numpy()[0].max(axis=(0, 2))
    worst = np.argsort(-drop)[:5]
    written = None
    try:
        metric.export_distogram(stats, distogram)
        written = distogram
    except Exception as e:  # matplotlib missing or a single frame
        written = f"not written: {e}"
    json.dump(
        {
            "jod": round(float(jod), 3),
            "display": "standard_fhd",
            "device": str(device),
            "frames": int(n),
            "worst_frames": [{"frame": int(i), "jod_drop": round(float(drop[i]), 3)} for i in worst],
            "distogram": written,
        },
        open(out_json, "w"),
    )


main()

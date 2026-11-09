"""Plain-Python helpers (PIL + numpy) used by the piece drivers. Run with python -I."""
import json, os, subprocess, sys
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

BLENDER = os.environ.get("BLENDER_EXE", "blender")  # Blender 5.3; set BLENDER_EXE if it is not on PATH
HERE = os.path.dirname(os.path.abspath(__file__))
STUDY = os.path.dirname(HERE)


def blender(script, *args, blend=None):
    cmd = [BLENDER, "-b", "--factory-startup", "--disable-autoexec"]
    if blend:
        cmd.append(blend)
    cmd += ["--python-exit-code", "1", "--python", os.path.join(HERE, script), "--", *map(str, args)]
    r = subprocess.run(cmd, capture_output=True, text=True, errors="replace")
    log = r.stdout + r.stderr
    if r.returncode != 0 or "Traceback" in log:
        print(log[-4000:])
        raise SystemExit(f"blender failed: {script} {args}")
    return log


def palette_from_id(id_png, id_json, concept_png):
    """Median concept colour (sRGB 0..1) over each object's visible pixels in the ID render."""
    ids = np.asarray(Image.open(id_png).convert("RGB"))[:, :, 0].astype(int)
    con = np.asarray(Image.open(concept_png).convert("RGB")).astype(float) / 255
    names = json.load(open(id_json))
    out, cover = {}, {}
    for k, n in names.items():
        m = ids == int(k)
        cover[n] = int(m.sum())
        if m.sum() > 20:
            out[n] = [float(v) for v in np.median(con[m], axis=0)]
    return out, cover


def unseen_fraction(png):
    """Share of non-background pixels rendered in the magenta 'concept never showed this' colour."""
    a = np.asarray(Image.open(png).convert("RGB")).astype(int)
    mag = (a[:, :, 0] > 230) & (a[:, :, 1] < 25) & (a[:, :, 2] > 230)
    bg = a.sum(2) < 6
    return round(float(mag.sum() / max(1, (~bg).sum())), 4)


def blend(a_png, b_png, out_png, alpha=0.5):
    a = Image.open(a_png).convert("RGB")
    b = Image.open(b_png).convert("RGB").resize(a.size)
    Image.blend(a, b, alpha).save(out_png)


def edge_overlay(concept_png, render_png, out_png):
    """Concept dimmed, render edges drawn in cyan: shows where blockout outlines miss concept outlines."""
    c = Image.open(concept_png).convert("L")
    r = Image.open(render_png).convert("RGB").resize(c.size)
    e = np.asarray(r.filter(ImageFilter.FIND_EDGES).convert("L")) > 20
    base = np.stack([np.asarray(c)] * 3, -1).astype(float) * 0.6
    base[e] = (0, 255, 255)
    Image.fromarray(base.astype(np.uint8)).save(out_png)


def contact_sheet(rows, out_png, cell_w=360, label_h=18):
    """rows: list of (row_label, [(cell_label, png_path), ...])"""
    ncol = max(len(r[1]) for r in rows)
    cells = []
    for _, items in rows:
        for _, p in items:
            if p and os.path.exists(p):
                im = Image.open(p)
                cells.append(im.size[1] / im.size[0])
    ratio = max(cells) if cells else 0.8
    cell_h = int(cell_w * ratio)
    W, H = 110 + ncol * cell_w, len(rows) * (cell_h + label_h)
    sheet = Image.new("RGB", (W, H), (28, 24, 32))
    d = ImageDraw.Draw(sheet)
    for ri, (rl, items) in enumerate(rows):
        y = ri * (cell_h + label_h)
        d.text((6, y + cell_h // 2), rl, fill=(240, 220, 200))
        for ci, (cl, p) in enumerate(items):
            x = 110 + ci * cell_w
            if p and os.path.exists(p):
                im = Image.open(p).convert("RGB")
                im.thumbnail((cell_w - 4, cell_h - 4))
                sheet.paste(im, (x + 2, y + 2))
            d.text((x + 4, y + cell_h + 2), cl, fill=(200, 200, 210))
    sheet.save(out_png)


def gif(frames, out_gif, ms=80, width=None):
    ims = []
    for p in frames:
        im = Image.open(p).convert("RGB")
        if width:
            im = im.resize((width, int(im.size[1] * width / im.size[0])), Image.LANCZOS)
        ims.append(im.convert("P", palette=Image.ADAPTIVE, colors=255))
    ims[0].save(out_gif, save_all=True, append_images=ims[1:], duration=ms, loop=0)


def strip(frames, out_png, n=8, width=260):
    idx = np.linspace(0, len(frames) - 1, n).round().astype(int)
    ims = [Image.open(frames[i]).convert("RGB") for i in idx]
    ims = [im.resize((width, int(im.size[1] * width / im.size[0])), Image.LANCZOS) for im in ims]
    s = Image.new("RGB", (width * n, ims[0].size[1]), (0, 0, 0))
    for i, im in enumerate(ims):
        s.paste(im, (i * width, 0))
    s.save(out_png)

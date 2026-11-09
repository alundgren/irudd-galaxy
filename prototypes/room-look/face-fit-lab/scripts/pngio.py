# Minimal PNG read/write with numpy and zlib, so the 2D steps run in Blender's bundled
# Python (which has numpy but not PIL). Reads 8-bit RGB/RGBA/grey, non-interlaced.
import struct
import zlib

import numpy as np

_CH = {0: 1, 2: 3, 4: 2, 6: 4}


def read(path):
    """Return an (h, w, 3) uint8 array (alpha dropped, grey expanded)."""
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat, w = 8, [], None
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, inter = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and inter == 0, "only 8-bit non-interlaced PNG"
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
        pos += 12 + n
    ch = _CH[ctype]
    raw = np.frombuffer(zlib.decompress(b"".join(idat)), dtype=np.uint8)
    stride = w * ch
    raw = raw.reshape(h, stride + 1)
    out = np.zeros((h, stride), dtype=np.int32)
    prev = np.zeros(stride, dtype=np.int32)
    for y in range(h):
        f, line = raw[y, 0], raw[y, 1:].astype(np.int32)
        if f == 0:
            cur = line
        elif f == 1:
            cur = line.copy()
            for x in range(ch, stride):
                cur[x] = (cur[x] + cur[x - ch]) & 255
        elif f == 2:
            cur = (line + prev) & 255
        elif f == 3:
            cur = line.copy()
            for x in range(stride):
                a = cur[x - ch] if x >= ch else 0
                cur[x] = (cur[x] + ((a + prev[x]) >> 1)) & 255
        elif f == 4:
            cur = line.copy()
            for x in range(stride):
                a = cur[x - ch] if x >= ch else 0
                b = prev[x]
                c = prev[x - ch] if x >= ch else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                cur[x] = (cur[x] + pr) & 255
        else:
            raise ValueError("bad filter")
        out[y] = cur
        prev = cur
    img = out.reshape(h, w, ch).astype(np.uint8)
    if ch == 1:
        img = np.repeat(img, 3, axis=2)
    elif ch == 2:
        img = np.repeat(img[:, :, :1], 3, axis=2)
    return img[:, :, :3]


def write(path, img):
    """Write an (h, w, 3) or (h, w) uint8/float array as RGB PNG, no metadata chunks."""
    a = np.asarray(img)
    if a.dtype != np.uint8:
        a = np.clip(a, 0, 255).round().astype(np.uint8)
    if a.ndim == 2:
        a = np.repeat(a[:, :, None], 3, axis=2)
    a = np.ascontiguousarray(a[:, :, :3])
    h, w = a.shape[:2]
    raw = np.concatenate([np.zeros((h, 1), np.uint8), a.reshape(h, w * 3)], axis=1).tobytes()

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 6)))
        f.write(chunk(b"IEND", b""))


def resize(img, w, h):
    """Area-average downscale / bilinear upscale, good enough for comparison sheets."""
    a = np.asarray(img, dtype=np.float32)
    sh, sw = a.shape[:2]
    if w <= sw and h <= sh:
        ys = np.linspace(0, sh, h + 1)
        xs = np.linspace(0, sw, w + 1)
        cs = np.cumsum(np.cumsum(np.pad(a, ((1, 0), (1, 0), (0, 0))), 0), 1)
        y0, y1 = ys[:-1], ys[1:]
        x0, x1 = xs[:-1], xs[1:]

        def S(yy, xx):
            yi = np.clip(yy, 0, sh)
            xi = np.clip(xx, 0, sw)
            yf, xf = np.floor(yi).astype(int), np.floor(xi).astype(int)
            return cs[yf][:, xf]
        tot = S(y1, x1) - S(y0, x1) - S(y1, x0) + S(y0, x0)
        area = np.outer(np.floor(y1) - np.floor(y0), np.floor(x1) - np.floor(x0))[:, :, None]
        return tot / np.maximum(area, 1)
    gy = (np.arange(h) + 0.5) * sh / h - 0.5
    gx = (np.arange(w) + 0.5) * sw / w - 0.5
    y0 = np.clip(np.floor(gy).astype(int), 0, sh - 1)
    x0 = np.clip(np.floor(gx).astype(int), 0, sw - 1)
    y1 = np.clip(y0 + 1, 0, sh - 1)
    x1 = np.clip(x0 + 1, 0, sw - 1)
    fy = np.clip(gy - y0, 0, 1)[:, None, None]
    fx = np.clip(gx - x0, 0, 1)[None, :, None]
    return (a[y0][:, x0] * (1 - fy) * (1 - fx) + a[y0][:, x1] * (1 - fy) * fx
            + a[y1][:, x0] * fy * (1 - fx) + a[y1][:, x1] * fy * fx)

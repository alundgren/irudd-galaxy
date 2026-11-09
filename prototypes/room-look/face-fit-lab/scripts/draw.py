# Drawing helpers for overlays and comparison sheets: markers, lines and a tiny 5x7
# bitmap font for labels. numpy only.
import numpy as np

_F = {
    "A": "01110100011000111111100011000110001", "B": "11110100011000111110100011000111110",
    "C": "01110100011000010000100001000101110", "D": "11110100011000110001100011000111110",
    "E": "11111100001000011110100001000011111", "F": "11111100001000011110100001000010000",
    "G": "01110100011000010111100011000101111", "H": "10001100011000111111100011000110001",
    "I": "01110001000010000100001000010001110", "J": "00111000100001000010000101001001100",
    "K": "10001100101010011000101001001010001", "L": "10000100001000010000100001000011111",
    "M": "10001110111010110101100011000110001", "N": "10001100011100110101100111000110001",
    "O": "01110100011000110001100011000101110", "P": "11110100011000111110100001000010000",
    "Q": "01110100011000110001101011001001101", "R": "11110100011000111110101001001010001",
    "S": "01111100001000001110000010000111110", "T": "11111001000010000100001000010000100",
    "U": "10001100011000110001100011000101110", "V": "10001100011000110001100010101000100",
    "W": "10001100011000110101101011010101010", "X": "10001100010101000100010101000110001",
    "Y": "10001100010101000100001000010000100", "Z": "11111000010001000100010001000011111",
    "0": "01110100011001110101110011000101110", "1": "00100011000010000100001000010001110",
    "2": "01110100010000100010001000100011111", "3": "11111000100010000010000011000101110",
    "4": "00010001100101010010111110001000010", "5": "11111100001111000001000011000101110",
    "6": "00110010001000011110100011000101110", "7": "11111000010001000100010000100001000",
    "8": "01110100011000101110100011000101110", "9": "01110100011000101111000010001001100",
    " ": "0" * 35, "-": "00000000000000011111000000000000000", ".": "0" * 30 + "01100",
    "+": "00000001000010011111001000010000000", "%": "11001110010001000100010001001110011",
    "/": "00001000010001000100010001000010000", ":": "00000011000110000000011000110000000",
    "(": "00010001000100001000010000010000010", ")": "01000001000001000010000100010001000",
    "=": "00000000001111100000111110000000000", "X2": "0" * 35,
}


def text(img, s, x, y, scale=2, color=(255, 255, 255), bg=(0, 0, 0)):
    """Draw upper-cased text with its top-left at (x, y)."""
    s = s.upper()
    cw = 6 * scale
    if bg is not None:
        x1 = min(img.shape[1], x + cw * len(s) + scale)
        y1 = min(img.shape[0], y + 8 * scale + scale)
        img[max(0, y - scale):y1, max(0, x - scale):x1] = bg
    for i, ch in enumerate(s):
        g = _F.get(ch, _F[" "])
        for r in range(7):
            for c in range(5):
                if g[r * 5 + c] == "1":
                    yy, xx = y + r * scale, x + i * cw + c * scale
                    img[yy:yy + scale, xx:xx + scale] = color
    return img


def line(img, p0, p1, color, w=1):
    n = int(max(abs(p1[0] - p0[0]), abs(p1[1] - p0[1]))) + 1
    for t in np.linspace(0, 1, n):
        x = int(round(p0[0] + (p1[0] - p0[0]) * t))
        y = int(round(p0[1] + (p1[1] - p0[1]) * t))
        img[max(0, y - w // 2):y + w - w // 2, max(0, x - w // 2):x + w - w // 2] = color


def cross(img, p, color, r=4, w=1):
    line(img, (p[0] - r, p[1]), (p[0] + r, p[1]), color, w)
    line(img, (p[0], p[1] - r), (p[0], p[1] + r), color, w)


def ring(img, p, color, r=4, w=1):
    for a in np.linspace(0, 2 * np.pi, 8 * r + 8):
        x, y = int(round(p[0] + r * np.cos(a))), int(round(p[1] + r * np.sin(a)))
        if 0 <= y < img.shape[0] and 0 <= x < img.shape[1]:
            img[max(0, y - w // 2):y + w - w // 2 + 1, max(0, x - w // 2):x + w - w // 2 + 1] = color


def hstack(imgs, gap=6, bg=(30, 24, 36)):
    h = max(i.shape[0] for i in imgs)
    out = []
    for i, im in enumerate(imgs):
        pad = np.full((h, im.shape[1], 3), bg, dtype=np.float32)
        pad[:im.shape[0]] = im
        out.append(pad)
        if i < len(imgs) - 1:
            out.append(np.full((h, gap, 3), bg, dtype=np.float32))
    return np.concatenate(out, 1)


def vstack(imgs, gap=6, bg=(30, 24, 36)):
    w = max(i.shape[1] for i in imgs)
    out = []
    for i, im in enumerate(imgs):
        pad = np.full((im.shape[0], w, 3), bg, dtype=np.float32)
        pad[:, :im.shape[1]] = im
        out.append(pad)
        if i < len(imgs) - 1:
            out.append(np.full((gap, w, 3), bg, dtype=np.float32))
    return np.concatenate(out, 0)


def labelled(img, label, scale=2):
    """Add a label strip above the image."""
    strip = np.full((10 * scale + 4, img.shape[1], 3), (30, 24, 36), dtype=np.float32)
    text(strip, label, 4, 3, scale, (240, 230, 210), None)
    return np.concatenate([strip, np.asarray(img, dtype=np.float32)], 0)

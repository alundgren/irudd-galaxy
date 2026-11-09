"""Mean brightness of the board slots in each render: does the board respond to the lamp?
usage: python -I light_response.py id.png id.json render...   (renders from the concept camera)"""
import json, sys
import numpy as np
from PIL import Image

ids = np.asarray(Image.open(sys.argv[1]).convert("RGB"))[:, :, 0].astype(int)
names = {n: int(k) for k, n in json.load(open(sys.argv[2])).items()}
rows = {r: np.isin(ids, [v for n, v in names.items() if n.startswith(f"board_slot_{r}")]) for r in range(6)}
for p in sys.argv[3:]:
    a = np.asarray(Image.open(p).convert("RGB").resize((ids.shape[1], ids.shape[0]))).astype(float) / 255
    L = a @ [0.2126, 0.7152, 0.0722]
    print(f"{p[-40:]:40s} board rows top->bottom", " ".join(f"{L[m].mean():.3f}" for m in rows.values()))

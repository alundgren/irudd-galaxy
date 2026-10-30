# Flight lab

A small top-down sandbox for trying ship flight models: five control schemes, live knobs and saved presets. Python + pygame-ce, self-contained in this folder.

## Run

From this folder, with [uv](https://docs.astral.sh/uv/) (macOS and Windows, same command):

```
uv run flight_lab.py
```

Or with pip and Python 3.10+:

```
# macOS
python3 -m venv .venv && source .venv/bin/activate && pip install -r requirements.txt && python flight_lab.py
# Windows (PowerShell)
py -m venv .venv; .venv\Scripts\Activate.ps1; pip install -r requirements.txt; python flight_lab.py
```

Headless check: `uv run flight_lab.py --selftest` prints PASS/FAIL for each scheme, docking and presets.

## Control schemes (keys 1-5 or the panel)

| # | Scheme | Controls |
|---|---|---|
| 1 | WASD screen | WASD/arrows fly in screen directions and turn in smooth arcs. Tap a side key to nudge the course; it holds until you release all keys. |
| 2 | Mouse-aim | Ship faces the cursor. W or right mouse thrusts, S brakes. |
| 3 | Tank | A/D rotate, W thrusts, S brakes. |
| 4 | Click to fly | Click or hold left mouse to fly there (Proto 1 v2 style). Right click stops, WASD takes over. |
| 5 | Hybrid | Ship faces the cursor, W/S forward/back, A/D strafe. |

Everywhere: Shift boosts, Space brakes, mouse wheel zooms, R tows you home, F12 or P saves a screenshot to `screenshots/`, Tab toggles the panel, Esc quits. On a MacBook trackpad, right click is a two-finger click.

## Knobs

Drag a slider or scroll over it; changes apply at once. **Inertia** runs from 0 (stops the moment you let go) to 1 (fully Newtonian, coasts forever). In between, sideways speed swings round into your new heading instead of vanishing, and coasting bleeds off. Defaults are about 20% faster than the overnight build (max speed 400, acceleration 1320).

The box in the top-right corner always shows the scheme, preset and every value, so a screenshot records what you liked.

## Presets

Starters in `presets/`: **light drift** (default), **snappy**, **heavy hauler**, **newtonian**, and **proto 1 v2** (the old zero-inertia feel, for comparison). Type a name in the panel and press Save or Enter to write `presets/<name>.json`; click a preset to load it.

## World

**Home** station is at the centre: fly into the green pad to dock, fly off to undock. Four markers sit 6k to 18k away (North Beacon, East Relay, Far Wreck, Deep Buoy). Edge arrows point to them, and reaching one shows the trip distance and time.

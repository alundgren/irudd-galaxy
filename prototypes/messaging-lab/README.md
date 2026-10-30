# Messaging lab

A small top-down game for comparing three ways NPCs talk to the player. Fly, mine ice, dock, trade and follow a tiny story, then switch mode and play it again. Python + pygame-ce, self-contained in this folder.

## Run

From this folder, with [uv](https://docs.astral.sh/uv/) (macOS and Windows, same command):

```
uv run messaging_lab.py
```

Or with pip and Python 3.10+:

```
# macOS
python3 -m venv .venv && source .venv/bin/activate && pip install -r requirements.txt && python messaging_lab.py
# Windows (PowerShell)
py -m venv .venv; .venv\Scripts\Activate.ps1; pip install -r requirements.txt; python messaging_lab.py
```

Headless check: `uv run messaging_lab.py --selftest` plays the story in all three modes and prints PASS/FAIL. Add `--shots <folder>` to save a screenshot of each step.

## Controls

| | |
|---|---|
| Fly | W thrust, S reverse, A/D turn, Space brake, Shift boost (x2) |
| Mine | Hold M or the left mouse button next to the pale ice rocks |
| Dock / undock | Fly into a station's pad / fly off |
| Talk | Click a bubble (or Enter) for the next line; click an option to choose |
| Panels | Click the corner icon. L: log or leads, B: station board, C: chronicle |
| Modes | F1, F2, F3 (each restarts the story) |
| Other | P or Esc: pause (controls and mode buttons are there), R: tow back to the Wharf, F12: screenshot |

Flight is locked to one setup: tank controls, max speed 300, acceleration 450, inertia 0.40, turn 200°/s, brake 350, auto-brake to a stop in 1.0 s, zoom 0.80, no camera lead.

## The world and story

You start docked at **Lantern Wharf**. Ilse Marrow welcomes you: Odo at **Kettle Dock** needs ice, the pale rocks just east (**Rimewater**) are ice, and Kettle Dock is straight down along the blue buoys. Fill the hold (20), dock at Kettle Dock and choose Deliver to Odo, Sell or Keep. Delivering 20 lights Odo's greenhouse ring. Tamsin Oddway explains tenders. Back at the Wharf, Ilse points you along the drifting lantern current to **Cinder Hook**, where Rook lights a lamp and buys ice. Orrin Vale offers a mining lens for 600 cr.

Same in every mode: nothing changes cargo or credits until you pick an option, and each pick shows a short receipt. Hints are literal and few, and flavour is kept to dialogue. There are no timers.

## What each mode is testing

1. **Dock talk + ship log.** Is dock talk plus a short log enough to remember what happened and where to go? NPCs speak in short click-to-advance bubbles above the station while docked; undocking stops them (anything unsaid is said next time). The **Log** icon opens one summary line per visit, with open offers and leads pinned at the top. Clicking a line replays that visit's bubbles.
2. **Leads board.** Is a sparse list of places, each with how to get there and who told you, a better guide than a log? Same dock bubbles, no log. **Leads** shows one card per place you've heard of, marked "heard of" or "been there". There are no markers or arrows in space.
3. **Letters + Chronicle.** Does splitting messages by kind help? Offers and hand-offs that don't need a person present arrive as letters on the station board you see while docked (Orrin's lens offer is a letter at the Wharf). World changes happen visibly in the world and get one line, with no speaker, in a separate **Chronicle**. NPCs later refer back to them.

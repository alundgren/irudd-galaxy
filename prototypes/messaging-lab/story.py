"""The little story, the economy and the three ways messages reach the player. No drawing in here.

Hard rule: cargo and credits change only in choose(), letter_action() and mine(), and each of
those runs only on an explicit player action (a click on an option, or holding the mine key).
"""
from dataclasses import dataclass, field

HOLD_CAP = 20
START_CREDITS = 260
LENS_PRICE = 600
ODO_PRICE = 10  # Odo pays for greenhouse ice
KETTLE_PRICE = 8  # the Kettle Dock market
ROOK_PRICE = 12  # Rook, far out at Cinder Hook
MINE_RATE = 2.0  # ice per second
LENS_MULT = 1.5
RING_NEEDS = 20  # ice Odo needs before the greenhouse ring lights

PEOPLE = {
    "ilse": "Ilse Marrow",
    "orrin": "Orrin Vale",
    "odo": "Odo Fenn",
    "tamsin": "Tamsin Oddway",
    "rook": "Rook",
}
STATION_NAMES = {"wharf": "Lantern Wharf", "kettle": "Kettle Dock", "hook": "Cinder Hook"}
MODES = {
    1: "Dock talk + ship log",
    2: "Leads board",
    3: "Letters + Chronicle",
}

# place id -> (name, how to find it, who told you)
PLACES = {
    "wharf": ("Lantern Wharf", "Your home dock. You started here.", None),
    "rimewater": ("Rimewater", "Pale ice rocks just east of the Wharf.", "Ilse"),
    "kettle": ("Kettle Dock", "Fly straight down from the Wharf, along the blue buoys.", "Ilse"),
    "hook": ("Cinder Hook", "Follow the lantern current east of the Wharf.", "Ilse"),
}

IDLE = {
    "wharf": [("ilse", "Welcome back."), ("ilse", "Quiet dock today."), ("ilse", "Safe flying.")],
    "kettle": [("odo", "Greenhouse says hello."), ("tamsin", "Prices are steady."), ("odo", "Mind the buoys.")],
    "hook": [("rook", "Quiet out here."), ("rook", "The lanterns keep coming."), ("rook", "Still here.")],
}


@dataclass
class Line:
    who: str | None  # person id, None for receipts
    text: str
    kind: str = "say"  # say | choice | receipt
    options: list = field(default_factory=list)  # [(label, action, enabled)]
    beat: str | None = None  # marked done once this line has been shown
    summary: str | None = None  # log clause added once this line has been shown
    on_show: object = None  # callable run once when the line first shows
    shown: bool = False


@dataclass
class Visit:
    station: str
    clauses: list = field(default_factory=list)
    lines: list = field(default_factory=list)  # Line copies, for replay

    def text(self):
        parts = [c[0].upper() + c[1:] if i else c for i, c in enumerate(self.clauses)]
        return f"{STATION_NAMES[self.station]}: " + ". ".join(parts) + "."


@dataclass
class Letter:
    key: str
    sender: str
    title: str
    body: list
    options: list = field(default_factory=list)  # [(label, action)]
    read: bool = False
    receipt: str | None = None  # set once the player acted on it


class Story:
    def __init__(self, mode):
        self.mode = mode
        self.credits = START_CREDITS
        self.ice = 0
        self.mine_progress = 0.0
        self.lens = False
        self.hold_full_seen = False
        self.delivered = 0
        self.ring_lit = False
        self.lamp_lit = False
        self.lens_state = None  # None, "open" or "bought"
        self.done = set()  # beats that have been fully shown
        self.visited = {"wharf"}
        self.heard = {"wharf": None}  # place -> who told you, in the order heard
        self.letters = {"wharf": [], "kettle": [], "hook": []}
        self.chronicle = []
        self.chronicle_unread = 0
        self.log = []
        self.visit = None
        self.queue = []
        self.docked_at = None
        self.dock_count = {"wharf": 0, "kettle": 0, "hook": 0}
        self.events = []  # things the view may want to animate: "ring", "lamp", "hold_full"

    # ------------------------------------------------------------ places
    def hear(self, place):
        if place not in self.heard:
            self.heard[place] = PLACES[place][2]

    def visit_place(self, place):
        self.visited.add(place)

    def leads(self):
        """[(name, how, who, been there)] for every place heard of, in the order heard."""
        return [(PLACES[p][0], PLACES[p][1], who, p in self.visited) for p, who in self.heard.items()]

    @property
    def ring_segments(self):
        return min(4, self.delivered * 4 // RING_NEEDS)

    @property
    def mine_rate(self):
        return MINE_RATE * (LENS_MULT if self.lens else 1.0)

    def pinned(self):
        """Open offers and leads for the top of the ship log: [(tag, text)]."""
        out = []
        if self.lens_state == "open":
            out.append(("Offer", f"Orrin's mining lens, {LENS_PRICE} cr, at Lantern Wharf. You mine half again as fast."))
        if "kettle" in self.heard and not self.ring_lit:
            left = RING_NEEDS - self.delivered
            out.append(("Open", f"Odo at Kettle Dock wants {left} ice for his greenhouse."))
        for p in ("kettle", "hook", "rimewater"):
            if p in self.heard and p not in self.visited:
                out.append(("Lead", f"{PLACES[p][0]}: {PLACES[p][1][0].lower()}{PLACES[p][1][1:]}"))
        return out

    # ------------------------------------------------------------ mining
    def mine(self, dt):
        """Called only while the player holds the mine key next to ice."""
        if self.ice >= HOLD_CAP:
            return
        self.mine_progress += self.mine_rate * dt
        while self.mine_progress >= 1 and self.ice < HOLD_CAP:
            self.ice += 1
            self.mine_progress -= 1
        if self.ice >= HOLD_CAP:
            self.mine_progress = 0.0
            if not self.hold_full_seen:
                self.hold_full_seen = True
                self.events.append("hold_full")

    # ------------------------------------------------------------ docking and talk
    def dock(self, sid):
        self.docked_at = sid
        self.dock_count[sid] += 1
        self.visited.add(sid)
        self.visit = Visit(sid)
        self.post_letters()
        self.queue = self.beats(sid)
        self._show()

    def undock(self):
        """Bubbles stop. Anything not yet said is said again next time; nothing is bought or sold."""
        if self.visit and self.visit.clauses:
            self.log.append(self.visit)
        self.visit = None
        self.queue = []
        self.docked_at = None

    @property
    def line(self):
        return self.queue[0] if self.queue else None

    def _show(self):
        ln = self.line
        if ln is None or ln.shown:
            return
        ln.shown = True
        if ln.on_show:
            ln.on_show()
        if ln.beat:
            self.done.add(ln.beat)
        if ln.summary and self.visit:
            self.visit.clauses.append(ln.summary)
        if self.visit:
            self.visit.lines.append(Line(ln.who, ln.text, ln.kind, [(o[0], o[1], False) for o in ln.options]))

    def advance(self):
        """Click on a speech bubble or receipt. Choices need an option instead."""
        if self.line and self.line.kind != "choice":
            self.queue.pop(0)
            self._show()

    def choose(self, action):
        ln = self.line
        if not ln or ln.kind != "choice":
            return
        match = [o for o in ln.options if o[1] == action and o[2]]
        if not match:
            return
        self.queue.pop(0)
        self.queue[0:0] = self._act(action)
        self._show()

    def _act(self, action):
        n = self.ice
        if action == "deliver":
            got = n * ODO_PRICE
            self.ice, self.credits, self.delivered = 0, self.credits + got, self.delivered + n
            out = [Line(None, f"Gave {n} ice. Got {got} cr.", "receipt", summary=f"gave Odo {n} ice, got {got} cr")]
            if self.delivered >= RING_NEEDS and not self.ring_lit:
                self.ring_lit = True
                self.events.append("ring")
                self.add_chronicle("The greenhouse ring at Kettle Dock is lit.")
                self.post_letters()
                out.append(Line("odo", "There. Look at the ring."))
                if self.mode != 3:
                    out.append(Line("odo", "Warm again. Thank you."))
            elif not self.ring_lit:
                out.append(Line("odo", f"Thank you. {RING_NEEDS - self.delivered} more and it lights."))
            else:
                out.append(Line("odo", "Thanks. It keeps the ring warm."))
            return out
        if action in ("sell_kettle", "sell_rook"):
            price = KETTLE_PRICE if action == "sell_kettle" else ROOK_PRICE
            got = n * price
            self.ice, self.credits = 0, self.credits + got
            to = "Rook " if action == "sell_rook" else ""
            return [Line(None, f"Sold {n} ice. Got {got} cr.", "receipt", summary=f"sold {to}{n} ice for {got} cr")]
        if action == "keep":
            return [Line(None, f"Kept {n} ice.", "receipt")]
        if action == "buy_lens":
            if self.credits < LENS_PRICE or self.lens:
                return []
            self.credits -= LENS_PRICE
            self.lens, self.lens_state = True, "bought"
            return [Line(None, f"Paid {LENS_PRICE} cr. Lens fitted.", "receipt",
                         summary=f"bought the mining lens for {LENS_PRICE} cr"),
                    Line("orrin", "Done. You'll feel it on the next rock.")]
        if action == "not_now":
            return [Line("orrin", "It'll keep.")]
        return []

    # ------------------------------------------------------------ what people say on docking
    def beats(self, sid):
        m3 = self.mode == 3
        out = []

        def beat(bid, lines, summary=None, when=True):
            """lines: (who, text) or (who, text, run when shown). Said again next docking if cut short."""
            if not when or bid in self.done:
                return
            lines = [Line(t[0], t[1], on_show=t[2] if len(t) > 2 else None) for t in lines]
            lines[-1].beat, lines[-1].summary = bid, summary
            out.extend(lines)

        if sid == "wharf":
            beat("welcome", [
                ("ilse", "Welcome to Lantern Wharf. I keep the dock."),
                ("ilse", "Odo at Kettle Dock needs ice for his greenhouse.", lambda: self.hear("kettle")),
                ("ilse", "The pale rocks just east of here are ice.", lambda: self.hear("rimewater")),
                ("ilse", "Kettle Dock is straight down. Follow the blue buoys."),
            ], "Ilse said Odo at Kettle Dock needs ice")
            beat("ring_news", [("ilse", "Heard a ring lit up at Kettle Dock."), ("ilse", "Odo will sleep warm.")],
                 when=m3 and self.ring_lit)
            beat("lamp_news", [("ilse", "Word is the old lamp at Cinder Hook burns again.")],
                 when=m3 and self.lamp_lit)
            beat("current", [
                ("ilse", "Rook trades out at Cinder Hook."),
                ("ilse", "Follow the lantern current east of here. It drifts there.", lambda: self.hear("hook")),
            ], "Ilse pointed to Cinder Hook along the lantern current", when=self.dock_count["kettle"] > 0)
            if not m3:
                out.extend(self.lens_offer())
        elif sid == "kettle":
            beat("odo_meet", [
                ("odo", "I'm Odo. That's my greenhouse ring."),
                ("odo", f"It's gone cold. {RING_NEEDS} ice would light it."),
            ])
            if self.ice > 0:
                n = self.ice
                out.append(Line("odo", f"Is that ice? The greenhouse needs it. I pay {ODO_PRICE} a block.", "choice", [
                    (f"Deliver to Odo ({n * ODO_PRICE} cr)", "deliver", True),
                    (f"Sell ({n * KETTLE_PRICE} cr)", "sell_kettle", True),
                    ("Keep", "keep", True),
                ]))
            beat("tender", [
                ("tamsin", "Tamsin Oddway. I run tenders."),
                ("tamsin", "A tender is a slow trading ship."),
                ("tamsin", "It waits at a dock for a good price, then sells."),
                ("tamsin", "None for hire yet. Just so you know."),
            ], "Tamsin explained tenders")
            beat("tamsin_lamp", [("tamsin", "Heard the lamp at Cinder Hook is lit. Trade will follow.")],
                 when=m3 and self.lamp_lit)
        elif sid == "hook":
            beat("rook_meet", [
                ("rook", "Few come out this far."),
                ("rook", "The current brought you. It brings most."),
                ("rook", "Here. I'll light the lamp for you.", self.light_lamp),
            ], "Rook lit the lamp")
            beat("rook_ring", [("rook", "Heard a ring lit up at Kettle Dock. Good.")], when=m3 and self.ring_lit)
            if self.ice > 0:
                n = self.ice
                out.append(Line("rook", f"Ice? I pay {ROOK_PRICE} a block.", "choice", [
                    (f"Sell to Rook ({n * ROOK_PRICE} cr)", "sell_rook", True),
                    ("Keep", "keep", True),
                ]))
        if not out:
            options = IDLE[sid]
            who, text = options[(self.dock_count[sid] - 1) % len(options)]
            out.append(Line(who, text))
        return out

    def lens_offer(self):
        """Orrin asks in person (modes 1 and 2): once after the first trip, then again whenever you can pay."""
        if self.lens or self.dock_count["kettle"] == 0:
            return []
        afford = self.credits >= LENS_PRICE
        buy = (f"Buy lens ({LENS_PRICE} cr)", "buy_lens", True) if afford else \
            (f"Buy lens (need {LENS_PRICE} cr)", "buy_lens", False)
        ask = Line("orrin", f"A mining lens is {LENS_PRICE} cr. You mine half again as fast.", "choice",
                   [buy, ("Not now", "not_now", True)])
        if self.lens_state is None:
            def opened():
                self.lens_state = "open"
            first = Line("orrin", "Orrin Vale, hullwright. Got a minute?", on_show=opened)
            ask.summary = f"Orrin offered a mining lens for {LENS_PRICE} cr"
            return [first, ask]
        if afford:
            return [Line("orrin", "Still want that lens?"), ask]
        return []

    def light_lamp(self):
        if not self.lamp_lit:
            self.lamp_lit = True
            self.events.append("lamp")
            self.add_chronicle("The lamp at Cinder Hook burns again.")

    # ------------------------------------------------------------ mode 3: letters and the chronicle
    def add_chronicle(self, text):
        if self.mode == 3:
            self.chronicle.append(text)
            self.chronicle_unread += 1

    def post_letters(self):
        if self.mode != 3:
            return
        have = {lt.key for box in self.letters.values() for lt in box}
        if "lens" not in have and self.dock_count["kettle"] > 0:
            self.lens_state = "open"
            self.letters["wharf"].append(Letter(
                "lens", "Orrin Vale, hullwright", "A mining lens",
                [f"I can fit a mining lens here at the Wharf. {LENS_PRICE} cr.",
                 "You mine half again as fast.", "Orrin"],
                [("Buy lens", "buy_lens"), ("Not now", "not_now")]))
        if "odo_thanks" not in have and self.ring_lit:
            self.letters["wharf"].append(Letter(
                "odo_thanks", "Odo Fenn, Kettle Dock", "The ring is lit",
                ["The greenhouse is warm again.", "Thank you for the ice.", "Odo"]))
        if "rook_price" not in have and "hook" in self.heard and "hook" not in self.visited:
            self.letters["kettle"].append(Letter(
                "rook_price", "Rook, Cinder Hook", "Ice wanted",
                [f"I pay {ROOK_PRICE} a block for ice.", "Out past the lanterns.", "Rook"]))

    def board(self, sid):
        return self.letters.get(sid, [])

    def unread(self, sid):
        return sum(not lt.read for lt in self.board(sid))

    def letter_action(self, sid, letter, action):
        """A click on a letter's button while docked at its station. Returns the receipt, if any."""
        if self.docked_at != sid or letter not in self.board(sid) or letter.receipt:
            return None
        if action == "buy_lens" and letter.key == "lens":
            if self.credits < LENS_PRICE or self.lens:
                return None
            self.credits -= LENS_PRICE
            self.lens, self.lens_state = True, "bought"
            letter.receipt = f"Paid {LENS_PRICE} cr. Lens fitted."
            return letter.receipt
        return None

# Being sheets

A being sheet says who someone in the module is: their past, their voice, what they want
and hide, and where their story goes. The sheets are writing for the GM and for writers.
The game never loads this folder, and nothing in a sheet changes play until a line,
script or entity is written from it.

Read a sheet before writing any line for that being, and before writing a scene that
involves them. A line that goes against a sheet is a defect in the line or the sheet;
settle which one before going on (below, "When a sheet and the module disagree").

## Who has a sheet

| Sheet | Being | Entity |
| --- | --- | --- |
| [`player.md`](player.md) | The player | `core.ship` (`engine.being` with `player = true`) |
| [`sefa.md`](sefa.md) | Sefa Lind, exchange keeper at Cinder Kiln | `core.kiln_trader` |
| [`mags.md`](mags.md) | Mags Ferrow, dockmaster at Cinder Kiln | `core.kiln_master` |
| [`hessa.md`](hessa.md) | Hessa Kell, skipper of Cold Kettle | `core.kettle_skipper` |

Sefa, Mags and Hessa are the core's anchors: the three who carry the long story for the
whole game. The other keepers have no sheets yet. Don't write one for them, and don't give
them a part in the long story, until the owner asks.

## What a sheet holds

Every sheet has the same sections, in this order:

1. **Backstory.** The past as it happened. It is the hidden truth: most of it the player
   never hears, and none of it is said in one go.
2. **Left open.** Gaps left on purpose for later writing. Don't fill a gap in passing; a
   gap is filled by the work that needs it, and then moves into the backstory.
3. **Traits and voice.** How they look, act and talk, with lines the core already has as
   samples.
4. **Want, need, fear.** Want is the outer goal the player can help or hinder. Need is
   the change their arc moves toward, which they may not see. Fear is what they avoid,
   and why they keep their secrets.
5. **Secrets.** Each with the truth, who else knows it, the plant (the first hint, often a
   line the core already has) and what reveals it: a place, a person, an object, or a
   beat that must come first.
6. **Relationships.** Each tie written from both sides: what this being makes of the
   other, and what the other makes of them. A tie between two anchors appears in both
   sheets and says the same thing in both.
7. **Arc beats.** Their story in order. Each beat says whether the core already plays it
   (with where) or it is still to be written, and what triggers it.
8. **Never says.** Lines and kinds of line they would never say. These are consistency
   rules for every writer.
9. **Kept true.** Every line the core already has that touches the sheet's story, and how
   the sheet keeps it true.

The player's sheet uses the same sections, but holds only the past (below).

## The player's sheet

The player has one fixed origin and makes no choice about it. Their sheet holds the past
only: the origin, ties to beings and places, and memories the character has that the
human doesn't yet. Each memory is released by a place, a person or an object, never by a
timer or a menu.

**The past never tells the player how to feel or what to choose.** A memory is a fact or
a thing seen, heard or smelled, never a feeling, a judgement or a pull toward a place.
"A stove with a kettle on a chain" is a memory. "You feel at home", "something tugs at
you" or "you have to find her" are not. What the past means is the human's to decide.
So the player's sheet leaves want, need, fear, voice and arc to the human, and its
"Never says" section lists what the game must never put in the player's mouth.

## Rules for the long story

- **No trust meter.** Nothing on screen counts how much someone likes or trusts the
  player. What they think shows only in what they say and do, and in what they are
  willing to tell.
- **Friction and discovery, no pointing arrows.** A memory or a secret is found by going
  somewhere, meeting someone or looking at something. Nothing points the way to it, and
  people's directions stay vague but correct, as everywhere in the game.
- **Plant, don't define.** The ring is a door. What is on its far side, who sent the
  child through, and what going through would mean are left open, so a later story can
  send the player through it to somewhere wholly new. Write nothing that rules that out.
- **A little at a time.** A secret comes out only after its plant has been heard, and
  never all at once. A secret once told is never told again, as the core already never
  asks or tells twice.
- **Both branches stand.** Cold Kettle's job ends either kept at Lantern Dock or mended at
  Cinder Kiln. Every later beat with Hessa must work after either.

## How later modules use the sheets

Each module keeps its own sheets in its own `beings/` folder, in this format, for the
beings it adds.

The core owns its long threads (below). A later module may read where a thread stands and
write beats around it: its own beings can hear of the ring, react to the player's past,
or mention Hessa. It never advances a core thread and never reveals a core secret the
core hasn't revealed yet. Advancing a thread is core work.

| Long thread | Anchors | What it is |
| --- | --- | --- |
| The door | Sefa, Mags | The ring by the glassite is one of a pair of doors. Where the other is, and what lies past either, is open. |
| The child in the ice | Hessa, Mags | The player came through the ring alone at about three. Hessa found and raised them, then sent them inward. |
| The boat in the floe | Hessa, Sefa | The lifeboat Hessa froze into a floe, and the canopy Sefa's kiln couldn't melt. |
| The chart in the tag | the player | The glass tag the player has always carried holds the map's blueprints: the minimap first, then the full map. |

## When a sheet and the module disagree

Every line already in the module stays true. Where a sheet contradicts the module's
README or a line in its files, change the sheet. Where the contradiction can't be
settled that way, write it down in the sheet's "Kept true" section and raise it with the
owner; don't change a line to fit a sheet unasked.

A sheet changes in the same PR as the line or beat that changes it: a beat written into
the module moves from "still to be written" to "plays" in the sheet, and a gap that gets
filled moves from "Left open" into the backstory.

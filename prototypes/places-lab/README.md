# places-lab

A throwaway prototype, like `prototypes/ui-lab`, that settles one question before the
engine builds scenes, cameras and talk: **how does a station become a place you are in?**
Only the findings carry over, in a REPORT.md at the end.

The ui-lab's verdict ("Where the player is" in `prototypes/ui-lab/REPORT.md`): a speech
bubble hangs off a building, not a person; opening the cradle cuts to another scene and
opening the office nudges the camera, and the two don't match. [RESEARCH.md](RESEARCH.md)
has what other games do, checked against sources.

## The place model (shared by every variant)

- **Places nest:** station > module > interior. A place has a name, a parent, the
  places it connects to, and where you arrive in it.
- **People are beings in places:** each has a name, a place, a state (at work, eating,
  away) and a point their speech and face come from. People move between places at story
  moments.
- **The player has a place.** Clicking a module means going there; the model says where
  the player is, and the view follows.
- Content (here, the lab's Luau) says which places and people the station has. The
  model leaves room for walking: places have floor positions, not only names.

The station is ours: the cradle, office, market and canteen, with Ilse, Orrin and a few
more people.

## The variants

Four choices, each switched live with keys, so any combination can be tried:

| Choice | Variants |
|---|---|
| Getting between places | cut with a title card · glide in and dissolve into the room · cutaway (the wall opens in place) · a threshold (doorway or short corridor) · honest menu over the dimmed station · walk (an avatar crosses the cutaway) |
| What a room is | a staged 3D set with a fixed camera and people in it · a flat staged scene in layers with text · the interior inside the station model |
| How people talk | bubble on the person · portrait with a name panel · the camera frames the speaker · a text column at the side |
| Life | people in states who move at story moments · changes visible when you come back |

Variants are named by technique. Don't copy any game's layout, screens or look.

## Order

1. **Spine:** the place model, the station with its people, and one baseline that plays
   end to end (cut, staged set, bubble on the person). Key switching, F8 notes tagged
   with the combination in use, and `--look`/`--shots` per combination. Each choice
   lives in its own module, so variants can be built in parallel.
2. **Fan out:** one agent per choice, on the integration branch `prototype/places-lab`.
3. **Play and report:** the owner plays the combinations; the report says what the
   engine's place model, cameras and talk should be.

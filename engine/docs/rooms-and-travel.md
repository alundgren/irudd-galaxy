# Rooms and travel

A room is a place with `engine.room { title, camera, fov }` and `engine.model`.
The model is a real 3D glTF set. Its floor, walls and furniture are ordinary triangles,
so a future walking camera can use the same set. Room sets use unit scale, no morph,
and the local origin; transforms belong in glTF. `engine.room.camera` names a node
(default `camera`), whose inherited pose looks down glTF -Z with +Y up. `fov` is the
vertical field of view in degrees (default 40). Other places still use the follow camera.

`galaxy_mesh::gltf::read_nodes` retains named mesh and empty nodes with inherited
transforms, in the same world axes as the triangles: glTF `(x,y,z)` becomes world
`(z,x,y)`. Empty nodes therefore carry camera poses, floor spots and being parts without
adding invisible geometry. Duplicate names in one model are an authoring error.

## Spots and beings

Declare the room's `engine.spot` entities as usual, with their place, name and layer.
For each one, put a `spot.<name>` node in the set. At new-session loading,
`sim::rooms::hydrate` imports its x/y into the ordinary world component and places
entities occupying that spot there. Those are recorded initial state, not view memory.
A marker must lie on its floor: ground level is zero; raised floors and ramps need
matching `engine.surface` data on that place and layer. The existing move, walk, ground
and save services continue to own positions. Beings without an occupied spot keep their
world position. A walk and a later walking camera need no new place representation.

`engine.room.title` supplies the travel title; other places fall back to their full ID.
The fixture models are simple authored glTF, with generic room and being names.

The view draws only the selected place's set and the entities located there. It snaps
rather than interpolating between unrelated coordinate spaces on a move. Model figures
stand at their world floor positions. Screens receive the existing anchor map, augmented
with these projected names:

- `place.<full-place-id>`: the place's origin.
- `spot.<name>`: a floor spot in the shown place.
- `being.<full-being-id>`: the top of that being's model.
- `being.<full-being-id>.<node-name>`: a model node such as `face`, transformed by the
  being's facing and scale.

Explicit `engine.anchor` publishers continue to work. Screens use the same `anchor`
property for all publishers. These projections and interpolation run in Rust.

## One trip, one presentation

Room-enabled worlds initialize `engine.travel` on their player being. Worlds without
rooms can opt in by putting this component on their player. Existing worlds and
recordings without it retain their old behavior and hashes. The component holds
`active`, `from`, `to`, `start_tick`, `reverse`, and the source spot and pose
(`from_spot`, `from_x`, `from_y`, `from_height`, `from_angle`, `from_layer`). After each simulation tick, the
travel service compares the player's place before and after the tick. A change starts
a new trip with those endpoints; another move replaces the trip. The world location
changes immediately, including when scripts change it directly. Non-player moves do
not take over the player's view.

The view's cut is derived from that saved trip and `(tick + alpha)`, not accumulated
frame time. Start, step, busy and finish use one path for all place changes. The first
presentation fades out for 0.18 s, holds a title card for 0.60 s, then fades in for
0.22 s. Leaving for an ancestor swaps the two fades: the same journey played backward.
During fade-out, the source place's camera, models and conditional screen are selected;
then the destination is selected beneath the opaque title. Screens still read current
world state, with the player's source location and pose restored in the drawing snapshot during fade-out.
The trip is presentation-independent: walking later can replace the cut and camera.

Play supplies `engine.place_arrived { entity, place, start_tick }` after 30 ticks, or
on any left click during a trip. That click is consumed; it cannot press a covered
button too. Multiple clicks on the same tick land once. This is a declared, recorded
world **input**, with source `travel`. The sim validates that it matches the active
trip and clears `active` before dispatching it to scripts. Scripts may subscribe with
`on["engine.place_arrived"]`; they cannot emit engine events themselves.

Headless runs never invent presentation arrivals. A scenario can supply the same input,
and replay applies the input that play actually recorded, including early clicks.
Zoom remains a presentation event. No per-frame Lua and no additional recording format
are needed. The end of the cut may be drawn before an arrival is supplied in a headless
run, but its pending arrival remains world state until the input lands.

## Check, audit and film

`galaxy-gm check` clones the world and enters every room to evaluate its conditional
screens at 1280x720. It checks normal widget readability and every floor spot, including
empty rooms and arrival spots. It projects each present being's
model vertices, and rejects offscreen figures and figures overlapped by a panel. The
check does not move the live player. Room camera/spot authoring failures report the
room and missing node or inconsistent floor. `guide rooms` describes the workflow.

F8 uses the existing moment metadata (tick, interpolation, camera, frame size and scale).
Audit replays the world and rebuilds the trip, room, anchors and screen from the embedded
module assets. Film uses the same scene path, with only visible things in its probes.
No GPU state, screenshot, live view or private timer is required to reconstruct a cut.

From the repository root, with the dev build selected:

```sh
galaxy-gm --build <name> check engine/tests/play/rooms/demo
galaxy --build <name> play --modules engine/tests/play/rooms --inputs engine/tests/play/rooms-inputs.toml --ticks 160
galaxy --build <name> sim replay <session>/recording.json
galaxy-gm --build <name> audit <session>/recording.json
galaxy-gm --build <name> film <session>/recording.json --from 0 --to 45 --size 1280x720 --out room-film
galaxy-gm --build <name> guide motion
```

The bot enters and leaves each room, clicks to land Room B early, and takes room and
mid-cut notes. `view/tests/rooms.rs` checks the same loop headlessly, arrival handlers,
save/load and replay hashes, fresh-view reconstruction, anchors and framing failures.
Mesh tests check inherited empty-node transforms and camera axes. GPU work remains in
manual play/audit/film, not normal test runs.

## Alternatives

A private travel timer would lose its phase on save, audit or view replacement. A second
copy of places inside the renderer would drift from scripts and walks. World trip data
and imported spot components use the existing save path instead. Baking the camera into
painted room layers would require new assets for walking; glTF sets keep that option
open. A general transition graph would add machinery to a deliberately temporary cut.

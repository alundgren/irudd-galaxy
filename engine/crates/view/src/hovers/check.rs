use super::*;
use std::collections::BTreeSet;

/// Bring each descendant through all its scroll ancestors, without dispatching events.
pub(super) fn scroll_states(
    boxes: &[Boxed],
    original: &crate::screens::Presentation,
) -> Vec<std::collections::BTreeMap<String, f32>> {
    fn walk<'a>(
        b: &'a Boxed,
        ancestors: &mut Vec<&'a Boxed>,
        original: &crate::screens::Presentation,
        out: &mut Vec<std::collections::BTreeMap<String, f32>>,
    ) {
        if !ancestors.is_empty() {
            let mut offsets = original.scroll.clone();
            let mut y = b.rect.center().y;
            for parent in ancestors.iter().rev() {
                let old = original
                    .scroll
                    .get(&parent.widget.path)
                    .copied()
                    .unwrap_or(0.0);
                let offset = (old + y - parent.rect.center().y)
                    .clamp(0.0, (parent.content_height - parent.rect.h).max(0.0));
                y -= offset - old;
                offsets.insert(parent.widget.path.clone(), offset);
            }
            if offsets != original.scroll && !out.contains(&offsets) {
                out.push(offsets);
            }
        }
        if b.widget.kind == "scroll" {
            ancestors.push(b);
        }
        for child in &b.children {
            walk(child, ancestors, original, out);
        }
        if b.widget.kind == "scroll" {
            ancestors.pop();
        }
    }
    let mut out = Vec::new();
    for b in boxes {
        walk(b, &mut Vec::new(), original, &mut out);
    }
    out
}

impl View {
    pub(super) fn collect_hovers(&mut self, world: &World, alpha: f32, size: Vec2) {
        let geometry = self.geometry(world, alpha);
        let state = world
            .entities
            .iter()
            .filter_map(|(id, c)| {
                c.get(galaxy_api::engine::TALK)
                    .filter(|t| t.fields["active"] == true)
                    .map(|t| format!("{id}:{}", text(&t.fields, "current")))
            })
            .collect::<Vec<_>>();
        let mut points = Vec::new();
        for b in self.screens.all() {
            // Box/clip/viewport edges partition visible rectangular coverage exactly.
            let mut xs = vec![b.rect.x, b.rect.x + b.rect.w];
            let mut ys = vec![b.rect.y, b.rect.y + b.rect.h];
            for r in self
                .screens
                .all()
                .into_iter()
                .flat_map(|b| [Some(b.rect), b.clip])
                .flatten()
                .chain([galaxy_render::Rect::new(0.0, 0.0, size.x, size.y)])
            {
                xs.extend(
                    [r.x, r.x + r.w]
                        .into_iter()
                        .filter(|x| *x > b.rect.x && *x < b.rect.x + b.rect.w),
                );
                ys.extend(
                    [r.y, r.y + r.h]
                        .into_iter()
                        .filter(|y| *y > b.rect.y && *y < b.rect.y + b.rect.h),
                );
            }
            xs.sort_by(f32::total_cmp);
            ys.sort_by(f32::total_cmp);
            let p = xs
                .windows(2)
                .flat_map(|x| {
                    ys.windows(2)
                        .map(move |y| Vec2::new((x[0] + x[1]) * 0.5, (y[0] + y[1]) * 0.5))
                })
                .find(|p| {
                    self.screens
                        .hit(*p)
                        .is_some_and(|w| w.path == b.widget.path)
                });
            if let Some(p) = p {
                points.push(p);
            }
            if galaxy_script::screens::anchored(&b.widget.kind) {
                points.push(b.rect.center());
            }
        }
        for p in points {
            let result = self.look_in(world, alpha, size, p, &geometry);
            self.keep_hover(result, &state);
        }
        // Model-less anchors can have a covered centre and an exposed edge too.
        let anchors: Vec<_> = self
            .screens
            .all()
            .into_iter()
            .filter(|b| galaxy_script::screens::anchored(&b.widget.kind))
            .filter(|b| {
                let (id, part) = binding(world, b.widget.anchor.as_deref().unwrap_or(""));
                id.as_ref()
                    .is_none_or(|id| !self.hover_uses_mesh(world, id, part.as_deref()))
            })
            .map(|b| (b.widget.path.clone(), b.rect))
            .collect();
        for (path, r) in anchors {
            'target: for y in r.y.max(0.0).floor() as i32..(r.y + r.h).min(size.y).ceil() as i32 {
                for x in r.x.max(0.0).floor() as i32..(r.x + r.w).min(size.x).ceil() as i32 {
                    let result = self.look_in(
                        world,
                        alpha,
                        size,
                        Vec2::new(x as f32 + 0.5, y as f32 + 0.5),
                        &geometry,
                    );
                    if result["widget"].as_str() == Some(&path) {
                        self.keep_hover(result, &state);
                        break 'target;
                    }
                }
            }
        }
        // Each named mesh part gets one actual reachable point. Triangle centres are
        // cheap candidates; a pixel scan finds the exposed sliver when those are covered.
        let Some(camera) = self.camera else {
            return;
        };
        let posed = crate::talk::Posed::new(camera, size, &geometry);
        let mut scans = std::mem::take(&mut self.part_scans);
        let (tiles, poses) = scans.for_poses(posed);
        let mut seen = BTreeSet::new();
        for g in &geometry {
            let model = text(&world.component(&g.id, MODEL).unwrap().fields, "model");
            let ranges = self.assets.parts.get(model).cloned().unwrap_or_default();
            for part in ranges {
                let scan = poses
                    .entry((
                        g.id.clone(),
                        model.to_owned(),
                        part.name.clone(),
                        (part.vertices.start, part.vertices.end),
                    ))
                    .or_default();
                let frame = Frame {
                    geometry: &geometry,
                    tiles,
                    camera,
                    size,
                };
                let found = self.part_point(scan, &frame, g, &part.vertices);
                if let Some(p) = found {
                    let result = self.look_in(world, alpha, size, p, &geometry);
                    let key = format!("{}:{}", g.id, part.name);
                    if seen.insert(key) {
                        self.keep_hover(result, &state);
                    }
                }
            }
        }
        self.part_scans = scans;
    }

    /// The first point that picks the part (`vertices` of `g`) where no widget is hovered:
    /// the part's triangle centres in order, then each pixel of its screen bounds row by
    /// row. `scan` keeps the ray work for these poses, so the same scan under other
    /// screens finds the same point without casting those rays again.
    fn part_point(
        &self,
        scan: &mut PartScan,
        frame: &Frame,
        g: &crate::talk::Geometry,
        vertices: &std::ops::Range<usize>,
    ) -> Option<Vec2> {
        let Frame {
            geometry,
            tiles,
            camera,
            size,
        } = *frame;
        // The triangles that pick as this part: a ray that hits none of them can't pick
        // it, so it is ruled out before the full nearest-hit search.
        let triangles: Vec<[u32; 3]> = g
            .mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|t| vertices.contains(&(t[0] as usize)))
            .copied()
            .collect();
        let mut candidates = Vec::new();
        let mut lo = size;
        let mut hi = Vec2::ZERO;
        for tri in &triangles {
            let corners = tri
                .iter()
                .map(|i| {
                    g.transform
                        .transform_point3(Vec3::from(g.mesh.vertices[*i as usize].position))
                })
                .collect::<Vec<_>>();
            for v in &corners {
                if let Some(p) = camera.world_to_screen(*v, size) {
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
            if let Some(p) =
                camera.world_to_screen(corners.iter().copied().sum::<Vec3>() / 3.0, size)
            {
                candidates.push(p);
            }
        }
        let (x0, x1) = (lo.x.max(0.0).floor() as i32, hi.x.min(size.x).ceil() as i32);
        let (y0, y1) = (lo.y.max(0.0).floor() as i32, hi.y.min(size.y).ceil() as i32);
        let width = (x1 - x0).max(0) as usize;
        let total = candidates.len() + width * (y1 - y0).max(0) as usize;
        let point = |i: usize| match candidates.get(i) {
            Some(p) => *p,
            None => {
                let j = i - candidates.len();
                Vec2::new(
                    (x0 + (j % width) as i32) as f32 + 0.5,
                    (y0 + (j / width) as i32) as f32 + 0.5,
                )
            }
        };
        let picks = |p: Vec2| {
            let (o, d) = camera.screen_ray(p, size);
            let (local_o, local_d) = g.local(o, d);
            if !g.reaches(local_o, local_d)
                || g.mesh.raycast_among(&triangles, local_o, local_d).is_none()
            {
                return false;
            }
            let tiles = tiles.get_or_init(|| Tiles::new(geometry, camera, size));
            let closest = geometry
                .iter()
                .enumerate()
                .filter_map(|(k, g)| {
                    let (o, d) = g.local(o, d);
                    if !g.reaches(o, d) {
                        return None;
                    }
                    let (t, v) = g.mesh.raycast_among(tiles.at(k, p), o, d)?;
                    Some((t, g, v))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            closest.is_some_and(|(_, hit, v)| hit.id == g.id && vertices.contains(&v))
        };
        let on_screen = |p: Vec2| p.x >= 0.0 && p.y >= 0.0 && p.x < size.x && p.y < size.y;
        let covered = |p: Vec2| self.screens.hover_hit(p).is_some();
        // What earlier scans left, in order: points that pick, and runs a widget covered
        // whose rays weren't cast. Points that don't pick (or are off screen) are gone.
        let mut kept = Vec::with_capacity(scan.points.len());
        let mut found = None;
        let mut earlier = std::mem::take(&mut scan.points).into_iter();
        for entry in earlier.by_ref() {
            match entry {
                Scanned::Picks(i) => {
                    kept.push(Scanned::Picks(i));
                    if !covered(point(i)) {
                        found = Some(point(i));
                        break;
                    }
                }
                Scanned::Covered(range) => {
                    let mut run = range.start..range.start;
                    for i in range.clone() {
                        let p = point(i);
                        if covered(p) {
                            run.end = i + 1;
                            continue;
                        }
                        push_run(&mut kept, &mut run, i + 1);
                        if picks(p) {
                            kept.push(Scanned::Picks(i));
                            found = Some(p);
                            run = i + 1..range.end;
                            break;
                        }
                    }
                    push_run(&mut kept, &mut run, range.end);
                    if found.is_some() {
                        break;
                    }
                }
            }
        }
        kept.extend(earlier);
        if found.is_none() {
            let mut run = scan.next..scan.next;
            while scan.next < total {
                let i = scan.next;
                scan.next += 1;
                let p = point(i);
                if on_screen(p) && covered(p) {
                    run.end = i + 1;
                    continue;
                }
                push_run(&mut kept, &mut run, i + 1);
                if on_screen(p) && picks(p) {
                    kept.push(Scanned::Picks(i));
                    found = Some(p);
                    break;
                }
            }
            push_run(&mut kept, &mut run, scan.next);
        }
        scan.points = kept;
        found
    }

    fn keep_hover(&mut self, mut result: Value, state: &[String]) {
        if result["widget"].is_null() && result["entity"].is_null() {
            return;
        }
        result["talk"] = json!(state);
        result["scroll"] = json!(self.screens.presentation.scroll);
        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}",
            result["place"],
            result["widget"],
            result["entity"],
            result["part"],
            result["hover"]["text"],
            result["hover"]["problems"],
            result["talk"]
        );
        if let Some(checks) = &mut self.hover_checks {
            checks.entry(key).or_insert(result);
        }
    }
}

/// What a part scan casts its rays through.
struct Frame<'a> {
    geometry: &'a [crate::talk::Geometry],
    /// Built on the first ray these poses cast.
    tiles: &'a std::cell::OnceCell<Tiles>,
    camera: galaxy_render::Camera,
    size: Vec2,
}

/// Ends a covered run, keeping it when not empty, and starts the next at `next`.
fn push_run(kept: &mut Vec<Scanned>, run: &mut std::ops::Range<usize>, next: usize) {
    if run.start < run.end {
        kept.push(Scanned::Covered(run.clone()));
    }
    *run = next..next;
}

/// A stretch of a part's scan order already looked at.
enum Scanned {
    /// The point at this index picks the part.
    Picks(usize),
    /// Points a widget covered when they were scanned; their rays weren't cast.
    Covered(std::ops::Range<usize>),
}

/// How far each part's scan got, and what it found, for one camera, frame size and set
/// of posed meshes ([`crate::talk::Posed`]). Whether a point picks a part doesn't depend
/// on the screens, only whether a widget covers it does, so every talk line, scroll
/// offset and silent pose over the same meshes reuses the rays. Kept for the last few.
#[derive(Default)]
pub(crate) struct PartScans {
    recent: Vec<(crate::talk::Posed, std::cell::OnceCell<Tiles>, Parts)>,
}

/// Scans per entity, model, part name and vertex range.
type Parts = std::collections::BTreeMap<(String, String, String, (usize, usize)), PartScan>;

#[derive(Default)]
pub(crate) struct PartScan {
    points: Vec<Scanned>,
    /// The first index in scan order not looked at yet.
    next: usize,
}

impl PartScans {
    const KEEP: usize = 8;

    fn for_poses(
        &mut self,
        posed: crate::talk::Posed,
    ) -> (&std::cell::OnceCell<Tiles>, &mut Parts) {
        match self.recent.iter().position(|(p, _, _)| *p == posed) {
            Some(i) => {
                let recent = self.recent.remove(i);
                self.recent.push(recent);
            }
            None => {
                if self.recent.len() >= Self::KEEP {
                    self.recent.remove(0);
                }
                self.recent
                    .push((posed, std::cell::OnceCell::new(), Parts::new()));
            }
        }
        let (_, tiles, parts) = self.recent.last_mut().unwrap();
        (tiles, parts)
    }
}

/// Screen tile size for [`Tiles`], in pixels.
const TILE: f32 = 32.0;

/// Each posed mesh's triangles by the screen tiles a ray through them can cross, so a
/// pixel's ray is tested only against triangles that can hit it. A triangle whose corners
/// all project in front of the camera projects inside its corners' screen bounds; padded,
/// those bounds give its tiles, and a triangle off screen is in none. Any other triangle
/// is in every tile. Each tile keeps mesh order, so the nearest hit and its tie-break are
/// the ones a ray against the whole mesh finds.
pub(crate) struct Tiles {
    columns: usize,
    rows: usize,
    /// Per mesh in geometry order, per tile row by row, the triangles.
    meshes: Vec<Vec<Vec<[u32; 3]>>>,
}

impl Tiles {
    fn new(geometry: &[crate::talk::Geometry], camera: galaxy_render::Camera, size: Vec2) -> Self {
        let columns = (size.x / TILE).ceil().max(1.0) as usize;
        let rows = (size.y / TILE).ceil().max(1.0) as usize;
        // Far more than rounding moves a projected hit.
        const PAD: f32 = 2.0;
        let meshes = geometry
            .iter()
            .map(|g| {
                let projected: Vec<Option<Vec2>> = g
                    .mesh
                    .vertices
                    .iter()
                    .map(|v| {
                        camera
                            .world_to_screen(
                                g.transform.transform_point3(Vec3::from(v.position)),
                                size,
                            )
                            .filter(|p| p.is_finite())
                    })
                    .collect();
                let mut tiles = vec![Vec::new(); columns * rows];
                for t in g.mesh.indices.as_chunks::<3>().0 {
                    let (c0, c1, r0, r1) = match t.map(|i| projected[i as usize]) {
                        [Some(a), Some(b), Some(c)] => {
                            let lo = a.min(b).min(c) - PAD;
                            let hi = a.max(b).max(c) + PAD;
                            if hi.x < 0.0 || hi.y < 0.0 || lo.x >= size.x || lo.y >= size.y {
                                continue;
                            }
                            let cell = |v: f32, n: usize| ((v / TILE).max(0.0) as usize).min(n - 1);
                            (
                                cell(lo.x, columns),
                                cell(hi.x, columns),
                                cell(lo.y, rows),
                                cell(hi.y, rows),
                            )
                        }
                        _ => (0, columns - 1, 0, rows - 1),
                    };
                    for r in r0..=r1 {
                        for c in c0..=c1 {
                            tiles[r * columns + c].push(*t);
                        }
                    }
                }
                tiles
            })
            .collect();
        Self {
            columns,
            rows,
            meshes,
        }
    }

    /// The triangles of the `mesh`th posed mesh a ray through on-screen `p` can hit.
    fn at(&self, mesh: usize, p: Vec2) -> &[[u32; 3]] {
        let cell = |v: f32, n: usize| ((v / TILE).max(0.0) as usize).min(n - 1);
        &self.meshes[mesh][cell(p.y, self.rows) * self.columns + cell(p.x, self.columns)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::talk::Geometry;
    use galaxy_api::{
        command::{Batch, Cause, Command},
        engine::{LOCATION, TRAVEL},
    };
    use galaxy_module::module::Module;
    use galaxy_render::{Projection, Rect};
    use galaxy_sim::Session;
    use std::path::Path;

    const SIZE: Vec2 = Vec2::new(1280.0, 720.0);

    /// The talk demo with the player in its second room, which has named parts.
    fn fixture() -> (Session, View) {
        let module =
            Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/talk/demo"))
                .unwrap();
        let mut session = Session::new(vec![module], 1).unwrap();
        session.step(&[]).unwrap();
        let registry = session.registry().clone();
        session
            .world
            .apply(
                &Batch {
                    cause: Cause::Engine {
                        service: "test".into(),
                    },
                    commands: vec![Command::Move {
                        entity: "talk.player".into(),
                        place: "talk.room_b".into(),
                        spot: None,
                        state: None,
                    }],
                },
                &registry,
            )
            .unwrap();
        let player = session.world.entities.get_mut("talk.player").unwrap();
        assert!(player.contains_key(LOCATION));
        player
            .get_mut(TRAVEL)
            .unwrap()
            .fields
            .insert("active".into(), false.into());
        galaxy_sim::services::ground(&mut session.world);
        let mut view = View::detached(session.modules(), 1);
        view.before_step(&session.world);
        view.scene(&session.world, 1.0, SIZE, 0.0);
        (session, view)
    }

    /// The nearest hit over every triangle of every mesh, as picking always cast it.
    fn full(geometry: &[Geometry], o: Vec3, d: Vec3) -> Option<(u32, usize, usize)> {
        geometry
            .iter()
            .enumerate()
            .filter_map(|(k, g)| {
                g.mesh
                    .raycast_vertex(
                        g.inverse.transform_point3(o),
                        g.inverse.transform_vector3(d),
                    )
                    .map(|(t, v)| (t, k, v))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(t, k, v)| (t.to_bits(), k, v))
    }

    /// Bounds and tiles skip only triangles a ray can't hit: every pixel's nearest hit,
    /// bit for bit, is the one the whole meshes give, for the scene camera and for a
    /// perspective camera inside the room, so some triangles reach behind it.
    #[test]
    fn tiles_and_bounds_find_the_nearest_hit_of_the_whole_meshes() {
        let (session, view) = fixture();
        let geometry = view.geometry(&session.world, 1.0);
        let scene_camera = view.camera.unwrap();
        let mut close = scene_camera;
        close.projection = Projection::Perspective {
            fov_y: 1.2,
            near: 0.05,
            far: 500.0,
        };
        // Inside the room, looking at its being.
        close.eye = Vec3::new(1.0, -1.0, 1.5);
        close.target = Vec3::new(-2.0, 0.5, 0.8);
        close.up = Vec3::Z;
        let behind = geometry.iter().any(|g| {
            g.mesh.vertices.iter().any(|v| {
                let p = g.transform.transform_point3(Vec3::from(v.position));
                close.world_to_screen(p, SIZE).is_none()
            })
        });
        assert!(
            behind,
            "some triangles should reach behind the close camera"
        );
        for camera in [scene_camera, close] {
            let tiles = Tiles::new(&geometry, camera, SIZE);
            let mut hits = 0;
            for y in (0..SIZE.y as i32).step_by(5) {
                for x in (0..SIZE.x as i32).step_by(5) {
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let (o, d) = camera.screen_ray(p, SIZE);
                    let expected = full(&geometry, o, d);
                    let tiled = geometry
                        .iter()
                        .enumerate()
                        .filter_map(|(k, g)| {
                            let (o, d) = g.local(o, d);
                            if !g.reaches(o, d) {
                                return None;
                            }
                            g.mesh
                                .raycast_among(tiles.at(k, p), o, d)
                                .map(|(t, v)| (t, k, v))
                        })
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .map(|(t, k, v)| (t.to_bits(), k, v));
                    assert_eq!(tiled, expected, "pixel {p}");
                    let bounded = geometry
                        .iter()
                        .enumerate()
                        .filter_map(|(k, g)| g.hit_vertex(o, d).map(|(t, v)| (t, k, v)))
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .map(|(t, k, v)| (t.to_bits(), k, v));
                    assert_eq!(bounded, expected, "pixel {p}");
                    hits += expected.is_some() as usize;
                }
            }
            assert!(hits > 1000, "the rays should hit the room: {hits}");
        }
    }

    /// Part scans kept from earlier screens over the same poses find the points a
    /// fresh scan finds, as panels come and go over the parts in any order.
    #[test]
    fn kept_part_scans_find_what_a_fresh_scan_finds_under_any_screens() {
        let (session, mut kept) = fixture();
        let world = &session.world;
        let original = kept.screens.boxes.clone();
        let cover = |rect: Rect| {
            let mut b = original[0].clone();
            b.children.clear();
            b.hidden = false;
            b.clip = None;
            b.rect = rect;
            b.widget.kind = "panel".into();
            b.widget.path = "test/cover".into();
            b
        };
        let (w, h) = (SIZE.x, SIZE.y);
        let screens = [
            vec![],
            vec![cover(Rect::new(0.0, 0.0, w * 0.5, h))],
            vec![cover(Rect::new(0.0, 0.0, w, h * 0.6))],
            vec![cover(Rect::new(0.0, 0.0, w, h))],
            vec![],
            vec![cover(Rect::new(w * 0.3, 0.0, w * 0.7, h))],
            vec![
                cover(Rect::new(0.0, 0.0, w, h * 0.45)),
                cover(Rect::new(0.0, h * 0.55, w, h * 0.45)),
            ],
            vec![cover(Rect::new(0.0, 0.0, w * 0.5, h))],
        ];
        let parts = |view: &mut View, extra: &[crate::screens::Boxed]| {
            view.screens.boxes = original.clone();
            view.screens.boxes.extend(extra.iter().cloned());
            view.hover_checks = Some(Default::default());
            view.collect_hovers(world, 1.0, SIZE);
            let found: Vec<Value> = view
                .hover_checks
                .take()
                .unwrap()
                .into_values()
                .filter(|r| !r["part"].is_null())
                .collect();
            found
        };
        let mut seen = Vec::new();
        for extra in &screens {
            let (_, mut fresh) = fixture();
            let expected = parts(&mut fresh, extra);
            assert_eq!(parts(&mut kept, extra), expected);
            seen.push(expected.len());
        }
        assert!(seen[0] > 0, "the room's parts should be found: {seen:?}");
        assert_eq!(seen[3], 0, "a full cover hides every part: {seen:?}");
        assert!(seen.iter().any(|n| *n > 0 && *n < seen[0]), "{seen:?}");
    }
}

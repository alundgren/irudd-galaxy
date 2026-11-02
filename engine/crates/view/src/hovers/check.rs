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
            if b.widget.kind == "hover" {
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
            .filter(|b| b.widget.kind == "hover")
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
        let mut seen = BTreeSet::new();
        for g in &geometry {
            let model = text(&world.component(&g.id, MODEL).unwrap().fields, "model");
            let ranges = self.assets.parts.get(model).cloned().unwrap_or_default();
            for part in ranges {
                let mut candidates = Vec::new();
                let mut lo = size;
                let mut hi = Vec2::ZERO;
                for tri in g
                    .mesh
                    .indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .filter(|t| part.vertices.contains(&(t[0] as usize)))
                {
                    let vertices = tri
                        .iter()
                        .map(|i| {
                            g.transform
                                .transform_point3(Vec3::from(g.mesh.vertices[*i as usize].position))
                        })
                        .collect::<Vec<_>>();
                    for v in &vertices {
                        if let Some(p) = camera.world_to_screen(*v, size) {
                            lo = lo.min(p);
                            hi = hi.max(p);
                        }
                    }
                    if let Some(p) =
                        camera.world_to_screen(vertices.iter().copied().sum::<Vec3>() / 3.0, size)
                    {
                        candidates.push(p);
                    }
                }
                let pixels = (lo.y.max(0.0).floor() as i32..hi.y.min(size.y).ceil() as i32)
                    .flat_map(|y| {
                        (lo.x.max(0.0).floor() as i32..hi.x.min(size.x).ceil() as i32)
                            .map(move |x| Vec2::new(x as f32 + 0.5, y as f32 + 0.5))
                    });
                for p in candidates.into_iter().chain(pixels) {
                    if p.x < 0.0
                        || p.y < 0.0
                        || p.x >= size.x
                        || p.y >= size.y
                        || self.screens.hit(p).is_some()
                    {
                        continue;
                    }
                    let (o, d) = camera.screen_ray(p, size);
                    let closest = geometry
                        .iter()
                        .filter_map(|g| {
                            g.mesh
                                .raycast_vertex(
                                    g.inverse.transform_point3(o),
                                    g.inverse.transform_vector3(d),
                                )
                                .map(|(t, v)| (t, g, v))
                        })
                        .min_by(|a, b| a.0.total_cmp(&b.0));
                    if closest
                        .is_some_and(|(_, hit, v)| hit.id == g.id && part.vertices.contains(&v))
                    {
                        let result = self.look_in(world, alpha, size, p, &geometry);
                        let key = format!("{}:{}", g.id, part.name);
                        if seen.insert(key) {
                            self.keep_hover(result, &state);
                        }
                        break;
                    }
                }
            }
        }
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

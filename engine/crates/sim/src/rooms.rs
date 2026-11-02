//! Import authored floor markers once into the ordinary place model. Saved worlds keep
//! these coordinates; views need no private copy of who occupies a spot.
use crate::World;
use galaxy_api::{
    Registry,
    command::{Batch, Cause, Command},
    engine::{LOCATION, MODEL, PLACE, POSITION, ROOM, SPOT, TRAVEL},
};
use galaxy_module::module::Module;
use serde_json::json;
use std::collections::BTreeMap;

pub fn hydrate<'a>(
    world: &mut World,
    modules: impl Iterator<Item = &'a Module>,
    registry: &Registry,
) -> Result<(), String> {
    let mut models = BTreeMap::new();
    for module in modules {
        for (path, text) in &module.files.0 {
            if let Some(name) = path
                .strip_prefix("models/")
                .and_then(|p| p.strip_suffix(".gltf"))
            {
                models.insert(format!("{}.{name}", module.id()), text);
            }
        }
    }
    let mut commands = Vec::new();
    for (id, c) in &world.entities {
        let Some(room) = c.get(ROOM) else {
            continue;
        };
        if !c.contains_key(PLACE) {
            return Err(format!("room `{id}` needs engine.place"));
        }
        let model = c
            .get(MODEL)
            .and_then(|m| m.fields["model"].as_str())
            .unwrap_or("");
        let text = models
            .get(model)
            .ok_or_else(|| format!("room `{id}` has no model `{model}`"))?;
        let model_fields = &c.get(MODEL).unwrap().fields;
        if model_fields["scale"].as_f64() != Some(1.0)
            || model_fields["morph"].as_f64() != Some(0.0)
            || c.get(POSITION).is_some_and(|p| {
                ["x", "y", "height", "angle"]
                    .iter()
                    .any(|k| p.fields[*k].as_f64().unwrap_or(0.0) != 0.0)
            })
        {
            return Err(format!(
                "room `{id}` sets use unit scale, no morph and the local origin; author transforms in glTF"
            ));
        }
        let nodes = galaxy_mesh::gltf::read_nodes(text).map_err(|e| format!("room `{id}`: {e}"))?;
        let camera = room.fields["camera"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("camera");
        let camera = nodes
            .get(camera)
            .ok_or_else(|| format!("room `{id}` lacks its camera node"))?;
        if camera.forward.cross(camera.up).length_squared() < 0.01 {
            return Err(format!("room `{id}` camera axes are parallel"));
        }
        let fov = room.fields["fov"].as_f64().unwrap_or(0.0);
        if !(1.0..120.0).contains(&fov) {
            return Err(format!("room `{id}` fov must be between 1 and 120 degrees"));
        }
        for (spot_id, components) in &world.entities {
            let Some(spot) = components.get(SPOT) else {
                continue;
            };
            if spot.fields["place"] != *id {
                continue;
            }
            let name = spot.fields["name"].as_str().unwrap_or("");
            let node = nodes
                .get(&format!("spot.{name}"))
                .ok_or_else(|| format!("room `{id}` lacks node `spot.{name}`"))?;
            let floor = crate::services::floor_height(
                world,
                id,
                spot.fields["layer"].as_i64().unwrap_or(0),
                node.position.x as f64,
                node.position.y as f64,
            );
            if (floor - node.position.z as f64).abs() > 0.001 {
                return Err(format!(
                    "room `{id}` node `spot.{name}` is not on its floor; author engine.surface for raised floors"
                ));
            }
            commands.push(Command::Patch {
                entity: spot_id.clone(),
                component: SPOT.into(),
                fields: json!({"x": node.position.x as f64, "y": node.position.y as f64})
                    .as_object()
                    .unwrap()
                    .clone(),
            });
            for (being, components) in &world.entities {
                let Some(loc) = components.get(LOCATION) else {
                    continue;
                };
                if loc.fields["place"] != *id || loc.fields["spot"] != *spot_id {
                    continue;
                }
                let mut p = components
                    .get(POSITION)
                    .map(|c| c.fields.clone())
                    .unwrap_or_default();
                p.extend(
                    json!({"x": node.position.x as f64, "y": node.position.y as f64,
                    "height": node.position.z as f64, "layer": spot.fields["layer"]})
                    .as_object()
                    .unwrap()
                    .clone(),
                );
                commands.push(Command::Put {
                    entity: being.clone(),
                    component: POSITION.into(),
                    fields: p,
                });
            }
        }
    }
    if world.entities.values().any(|c| c.contains_key(ROOM))
        && let Some((id, _)) = crate::travel::player(world)
        && world.component(id, TRAVEL).is_none()
    {
        commands.push(Command::Put {
            entity: id.into(),
            component: TRAVEL.into(),
            fields: Default::default(),
        });
    }
    world.apply(
        &Batch {
            cause: Cause::Engine {
                service: "room_markers".into(),
            },
            commands,
        },
        registry,
    )
}

//! Reads shared by the save-file commands and the live socket: the world by path, a
//! store query, history facts. Errors are `(exit code, message)`; 4 is bad input.

use galaxy_contract::history::HistoryQuery;
use galaxy_core::History;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

type ReadResult = Result<Value, (u8, String)>;

fn bad(message: impl Into<String>) -> (u8, String) {
    (4, message.into())
}

/// Splits a path the way `--path` takes it: slash-separated (`stations/station.kettle_dock`),
/// a JSON pointer (`/stations/...`), or dotted (`stations.station.kettle_dock`). IDs
/// contain dots, so a dotted path is resolved against the JSON: at each level the
/// longest run of segments that names an existing key wins.
pub fn resolve<'v>(root: &'v Value, path: &str) -> Result<&'v Value, String> {
    let path = path.trim();
    if path.is_empty() || path == "/" || path == "." {
        return Ok(root);
    }
    let missing = || format!("no `{path}` in the world state");
    if path.contains('/') {
        let pointer: String = if path.starts_with('/') {
            path.to_owned()
        } else {
            path.split('/')
                .map(|seg| format!("/{}", seg.replace('~', "~0")))
                .collect()
        };
        return root.pointer(&pointer).ok_or_else(missing);
    }
    let segs: Vec<&str> = path.split('.').collect();
    let mut at = root;
    let mut i = 0;
    while i < segs.len() {
        match at {
            Value::Object(map) => {
                let found = (i + 1..=segs.len())
                    .rev()
                    .find_map(|j| map.get(&segs[i..j].join(".")).map(|v| (v, j)));
                let (v, j) = found.ok_or_else(missing)?;
                at = v;
                i = j;
            }
            Value::Array(items) => {
                let n: usize = segs[i].parse().map_err(|_| missing())?;
                at = items.get(n).ok_or_else(missing)?;
                i += 1;
            }
            _ => return Err(missing()),
        }
    }
    Ok(at)
}

/// `state get`.
pub fn state_get(world: &Value, path: Option<&str>) -> ReadResult {
    match path {
        None => Ok(world.clone()),
        Some(p) => resolve(world, p).cloned().map_err(bad),
    }
}

/// `state query`: records of one store, filtered.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StateQuery {
    /// A top-level store of the world: `stations`, `ships`, `npcs`, `markets`, `rocks`,
    /// `rumors`, `factions`, `regions`, `lanes`, `packs` ...
    pub store: String,
    /// `field=value` filters, all of which must hold. `field` is a path inside the
    /// record (dotted or slash-separated); `value` matches the field's JSON value, or
    /// its text for strings. `field~text` matches a substring.
    #[serde(rename = "where")]
    pub filters: Vec<String>,
    /// Only these fields of each record (paths as above); every field without.
    pub fields: Vec<String>,
    pub limit: Option<usize>,
}

fn matches(record: &Value, filter: &str) -> Result<bool, String> {
    let (field, want, contains) = if let Some((f, v)) = filter.split_once('=') {
        (f, v, false)
    } else if let Some((f, v)) = filter.split_once('~') {
        (f, v, true)
    } else {
        return Err(format!(
            "filter `{filter}` should be `field=value` or `field~text`"
        ));
    };
    let Ok(have) = resolve(record, field.trim()) else {
        return Ok(false);
    };
    let want = want.trim();
    Ok(match have {
        Value::String(s) if contains => s.contains(want),
        Value::String(s) => s == want,
        _ if contains => have.to_string().contains(want),
        other => serde_json::from_str::<Value>(want).is_ok_and(|w| w == *other),
    })
}

pub fn state_query(world: &Value, q: &StateQuery) -> ReadResult {
    if q.store.is_empty() {
        return Err(bad("name a store, e.g. `state query stations`"));
    }
    let store = resolve(world, &q.store).map_err(|_| {
        let stores: Vec<&str> = world
            .as_object()
            .map(|m| m.keys().map(String::as_str).collect())
            .unwrap_or_default();
        bad(format!(
            "no store `{}`; the world has {}",
            q.store,
            stores.join(", ")
        ))
    })?;
    let records: Vec<(String, &Value)> = match store {
        Value::Object(m) => m.iter().map(|(k, v)| (k.clone(), v)).collect(),
        Value::Array(a) => a
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        _ => return Err(bad(format!("`{}` is not a store", q.store))),
    };
    let mut rows = Vec::new();
    let mut count = 0;
    for (key, record) in records {
        let mut keep = true;
        for f in &q.filters {
            if !matches(record, f).map_err(bad)? {
                keep = false;
                break;
            }
        }
        if !keep {
            continue;
        }
        count += 1;
        if q.limit.is_some_and(|n| rows.len() >= n) {
            continue;
        }
        let row = if q.fields.is_empty() {
            record.clone()
        } else {
            let mut m = Map::new();
            for f in &q.fields {
                if let Ok(v) = resolve(record, f) {
                    m.insert(f.clone(), v.clone());
                }
            }
            Value::Object(m)
        };
        rows.push(json!({ "key": key, "record": row }));
    }
    Ok(json!({ "store": q.store, "count": count, "rows": rows }))
}

/// `history query` on a loaded history.
pub fn history_query(history: &History, q: &HistoryQuery) -> Value {
    let facts = history.find(q);
    json!({ "count": facts.len(), "facts": facts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotted_paths_follow_ids() {
        let w = json!({
            "stations": { "station.kettle_dock": { "build": { "stage": 2 } } },
            "list": [ { "a": 1 } ],
        });
        assert_eq!(
            resolve(&w, "stations.station.kettle_dock.build.stage").unwrap(),
            &json!(2)
        );
        assert_eq!(
            resolve(&w, "stations/station.kettle_dock/build").unwrap(),
            &json!({ "stage": 2 })
        );
        assert_eq!(resolve(&w, "list.0.a").unwrap(), &json!(1));
        assert!(resolve(&w, "stations.station.nope").is_err());
        assert_eq!(resolve(&w, "").unwrap(), &w);
    }

    #[test]
    fn queries_filter_and_pick_fields() {
        let w = json!({ "npcs": {
            "npc.ilse": { "name": "Ilse", "location": "station.lantern_wharf", "role": "dockmaster" },
            "npc.odo": { "name": "Odo", "location": "station.kettle_dock", "role": "grower" },
        }});
        let q = StateQuery {
            store: "npcs".into(),
            filters: vec!["location=station.kettle_dock".into()],
            fields: vec!["name".into()],
            limit: None,
        };
        let out = state_query(&w, &q).unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["rows"][0]["key"], "npc.odo");
        assert_eq!(out["rows"][0]["record"], json!({ "name": "Odo" }));
        let q = StateQuery {
            store: "npcs".into(),
            filters: vec!["role~dock".into()],
            ..StateQuery::default()
        };
        assert_eq!(state_query(&w, &q).unwrap()["count"], 1);
        assert!(state_query(&w, &StateQuery::default()).is_err());
    }
}

use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Keep measurements intact internally (including numeric check paths). Compact only
/// at the output boundary, so --detail changes presentation, never the analysis.
pub(super) fn compact(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(peaks) = object
                .get_mut("narrow_resonances")
                .and_then(Value::as_array_mut)
            {
                let total = peaks.len();
                peaks.sort_by(|a, b| {
                    b["level_dbfs"]
                        .as_f64()
                        .unwrap()
                        .total_cmp(&a["level_dbfs"].as_f64().unwrap())
                });
                peaks.truncate(12);
                object.insert("narrow_resonances_total".into(), json!(total));
                object.insert(
                    "narrow_resonances_detail".into(),
                    json!("Strongest 12 by level_dbfs; --detail lists every detected peak"),
                );
            }
            for (key, value) in object {
                if ["timeline", "momentary", "short_term", "stereo", "windows"]
                    .contains(&key.as_str())
                    && let Some(windows) = value.as_array()
                {
                    let mut ranges: BTreeMap<String, (f64, f64)> = BTreeMap::new();
                    for window in windows {
                        if let Some(fields) = window.as_object() {
                            for (field, value) in fields {
                                if let Some(v) = value.as_f64() {
                                    let range = ranges.entry(field.clone()).or_insert((v, v));
                                    range.0 = range.0.min(v);
                                    range.1 = range.1.max(v);
                                }
                            }
                        }
                    }
                    *value = json!({"window_count": windows.len(), "ranges": ranges.into_iter()
                        .map(|(k,(min,max))| (k,json!({"min":min,"max":max}))).collect::<serde_json::Map<String,Value>>(),
                        "detail": "Use --detail for every window"});
                } else if key == "findings" && value.is_array() {
                    let mut grouped: BTreeMap<String, Value> = BTreeMap::new();
                    for finding in value.as_array().unwrap() {
                        // Keep separate bus pairs; the representative contains numeric evidence.
                        let key = format!(
                            "{}:{}:{}:{}",
                            finding["kind"],
                            finding["value"]["a"],
                            finding["value"]["b"],
                            finding["value"]["value"]
                        );
                        let entry = grouped.entry(key).or_insert_with(|| {
                            let mut entry = finding.clone();
                            entry["occurrences"] = json!(0);
                            entry["evidence_detail"] = json!("value is the first occurrence; start_s/end_s span all occurrences, not necessarily continuous. Use --detail for each finding.");
                            entry
                        });
                        entry["occurrences"] = json!(entry["occurrences"].as_u64().unwrap() + 1);
                        entry["start_s"] = json!(
                            entry["start_s"]
                                .as_f64()
                                .unwrap()
                                .min(finding["start_s"].as_f64().unwrap())
                        );
                        entry["end_s"] = json!(
                            entry["end_s"]
                                .as_f64()
                                .unwrap()
                                .max(finding["end_s"].as_f64().unwrap())
                        );
                    }
                    *value = json!(grouped.into_values().collect::<Vec<_>>());
                } else {
                    compact(value);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(compact),
        _ => {}
    }
}

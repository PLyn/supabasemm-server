use crate::shared::models::{DiffEntry, ProjectConfig};
use leptos::prelude::*;
use std::collections::HashMap;

use serde_json::{Map, Value};

pub async fn json_diff(
    config_type: String,
    source_value: Value,
    dest_value: Value,
) -> Result<Option<ProjectConfig>, ServerFnError> {
    let diff_entries = calculate_diff(&config_type, &source_value, &dest_value)?;

    if diff_entries.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ProjectConfig {
            name: config_type,
            diffs: diff_entries,
        }))
    }
}

fn calculate_diff(
    config_type: &str,
    source: &Value,
    dest: &Value,
) -> Result<Vec<DiffEntry>, ServerFnError> {
    let mut diff_entries = Vec::new();

    // Pre-filter arrays if this is Secrets config
    if config_type == "Secrets" {
        if let (Value::Array(src_arr), Value::Array(dst_arr)) = (source, dest) {
            // Filter out SUPABASE_ secrets before diffing
            let filtered_src: Vec<Value> = src_arr
                .iter()
                .filter(|v| !is_supabase_secret(v))
                .cloned()
                .collect();
            let filtered_dst: Vec<Value> = dst_arr
                .iter()
                .filter(|v| !is_supabase_secret(v))
                .cloned()
                .collect();

            let filtered_src_value = Value::Array(filtered_src);
            let filtered_dst_value = Value::Array(filtered_dst);
            diff_values(
                "",
                &filtered_src_value,
                &filtered_dst_value,
                &mut diff_entries,
            );
        } else {
            diff_values("", source, dest, &mut diff_entries);
        }
    } else {
        diff_values("", source, dest, &mut diff_entries);
    }

    Ok(diff_entries)
}

fn is_supabase_secret(value: &Value) -> bool {
    if let Value::Object(obj) = value {
        if let Some(Value::String(name)) = obj.get("name") {
            return name.starts_with("SUPABASE_");
        }
    }
    false
}

fn diff_values(path: &str, source: &Value, dest: &Value, diffs: &mut Vec<DiffEntry>) {
    use Value::*;

    match (source, dest) {
        (Array(src), Array(dst)) => diff_arrays(path, src, dst, diffs),
        (Object(src), Object(dst)) => diff_objects(path, src, dst, diffs),
        _ if source != dest => {
            diffs.push(DiffEntry {
                key: if path.is_empty() { "root" } else { path }.to_string(),
                source_value: format_value(source),
                dest_value: format_value(dest),
            });
        }
        _ => {} // Values are equal
    }
}

fn diff_arrays(path: &str, src: &[Value], dst: &[Value], diffs: &mut Vec<DiffEntry>) {
    let src_map = to_id_map(src);
    let dst_map = to_id_map(dst);

    match (src_map, dst_map) {
        (Some(src_ids), Some(mut dst_ids)) => {
            diff_by_id(path, &src_ids, &mut dst_ids, diffs);
        }
        (Some(src_ids), None) => {
            for (id, val) in src_ids {
                diffs.push(DiffEntry {
                    key: format!(
                        "{}{}id:{}",
                        path,
                        if path.is_empty() { "" } else { "." },
                        id
                    ),
                    source_value: format_value(val),
                    dest_value: "null".to_string(),
                });
            }
        }
        (None, Some(dst_ids)) => {
            for (id, val) in dst_ids {
                diffs.push(DiffEntry {
                    key: format!(
                        "{}{}id:{}",
                        path,
                        if path.is_empty() { "" } else { "." },
                        id
                    ),
                    source_value: "null".to_string(),
                    dest_value: format_value(val),
                });
            }
        }
        (None, None) => {
            diff_by_index(path, src, dst, diffs);
        }
    }
}

fn to_id_map(arr: &[Value]) -> Option<HashMap<String, &Value>> {
    let mut map = HashMap::new();
    let mut has_ids = false;

    for item in arr {
        if let Value::Object(obj) = item {
            if let Some(Value::String(id)) = obj.get("id") {
                map.insert(id.clone(), item);
                has_ids = true;
            }
        }
    }

    if has_ids {
        Some(map)
    } else {
        None
    }
}

fn diff_by_id(
    path: &str,
    src_map: &HashMap<String, &Value>,
    dst_map: &mut HashMap<String, &Value>,
    diffs: &mut Vec<DiffEntry>,
) {
    for (id, src_val) in src_map {
        let item_path = format!(
            "{}{}id:{}",
            path,
            if path.is_empty() { "" } else { "." },
            id
        );

        if let Some(dst_val) = dst_map.remove(id) {
            diff_values(&item_path, src_val, &dst_val, diffs);
        } else {
            diffs.push(DiffEntry {
                key: item_path,
                source_value: format_value(src_val),
                dest_value: "null".to_string(),
            });
        }
    }

    for (id, dst_val) in dst_map.iter() {
        diffs.push(DiffEntry {
            key: format!(
                "{}{}id:{}",
                path,
                if path.is_empty() { "" } else { "." },
                id
            ),
            source_value: "null".to_string(),
            dest_value: format_value(dst_val),
        });
    }
}

#[cfg(feature = "ssr")]
fn diff_by_index(path: &str, src: &[Value], dst: &[Value], diffs: &mut Vec<DiffEntry>) {
    let max_len = src.len().max(dst.len());

    for i in 0..max_len {
        let item_path = format!("{}[{}]", path, i);

        match (src.get(i), dst.get(i)) {
            (Some(s), Some(d)) => {
                if s.is_object() && d.is_object() && s != d {
                    diffs.push(DiffEntry {
                        key: item_path,
                        source_value: format_value(s),
                        dest_value: format_value(d),
                    });
                } else if !s.is_object() || !d.is_object() {
                    diff_values(&item_path, s, d, diffs);
                }
            }
            (Some(s), None) => diffs.push(DiffEntry {
                key: item_path,
                source_value: format_value(s),
                dest_value: "null".to_string(),
            }),
            (None, Some(d)) => diffs.push(DiffEntry {
                key: item_path,
                source_value: "null".to_string(),
                dest_value: format_value(d),
            }),
            _ => {}
        }
    }
}

fn diff_objects(
    path: &str,
    src: &Map<String, Value>,
    dst: &Map<String, Value>,
    diffs: &mut Vec<DiffEntry>,
) {
    for (key, src_val) in src {
        let field_path = if path.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", path, key)
        };

        match dst.get(key) {
            Some(dst_val) => diff_values(&field_path, src_val, dst_val, diffs),
            None => diffs.push(DiffEntry {
                key: field_path,
                source_value: format_value(src_val),
                dest_value: "null".to_string(),
            }),
        }
    }

    for (key, dst_val) in dst {
        if !src.contains_key(key) {
            let field_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", path, key)
            };
            diffs.push(DiffEntry {
                key: field_path,
                source_value: "null".to_string(),
                dest_value: format_value(dst_val),
            });
        }
    }
}

fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_object_diff() {
        let source: Value = serde_json::from_str(r#"{"a": 1, "b": 2}"#).unwrap();
        let dest: Value = serde_json::from_str(r#"{"a": 1, "b": 3, "c": 4}"#).unwrap();

        let result = json_diff("test".to_string(), source, dest).await.unwrap();
        let config = result.unwrap();

        assert_eq!(config.diffs.len(), 2); // b changed, c added
        assert!(config
            .diffs
            .iter()
            .any(|d| d.key == "b" && d.dest_value == "3"));
        assert!(config
            .diffs
            .iter()
            .any(|d| d.key == "c" && d.source_value == "null"));
    }

    #[tokio::test]
    async fn test_edge_functions_diff() {
        let source = r#"[
            {"id": "func1", "version": 1},
            {"id": "func2", "version": 1}
        ]"#;
        let dest = r#"[]"#;

        let source_value: Value = serde_json::from_str(source).unwrap();
        let dest_value: Value = serde_json::from_str(dest).unwrap();

        let result = json_diff("test".to_string(), source_value, dest_value)
            .await
            .unwrap();
        let config = result.unwrap();

        assert!(!config.diffs.iter().any(|d| d.key == "length"));
        assert!(config.diffs.iter().any(|d| d.key == "id:func1"));
        assert!(config.diffs.iter().any(|d| d.key == "id:func2"));
    }

    #[tokio::test]
    async fn test_no_diff() {
        let source = r#"{"a": 1, "b": "test", "c": true}"#;
        let dest = r#"{"a": 1, "b": "test", "c": true}"#;

        let source_value: Value = serde_json::from_str(source).unwrap();
        let dest_value: Value = serde_json::from_str(dest).unwrap();

        let result = json_diff("test".to_string(), source_value, dest_value)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_nested_object_diff() {
        let source = r#"{
            "user": {
                "name": "John",
                "age": 30,
                "address": {
                    "street": "123 Main St",
                    "city": "Boston"
                }
            }
        }"#;
        let dest = r#"{
            "user": {
                "name": "John",
                "age": 31,
                "address": {
                    "street": "123 Main St",
                    "city": "New York",
                    "zip": "10001"
                }
            }
        }"#;

        let source_value: Value = serde_json::from_str(source).unwrap();
        let dest_value: Value = serde_json::from_str(dest).unwrap();

        let result = json_diff("test".to_string(), source_value, dest_value)
            .await
            .unwrap();
        let config = result.unwrap();

        assert_eq!(config.diffs.len(), 3);
        assert!(config
            .diffs
            .iter()
            .any(|d| d.key == "user.age" && d.dest_value == "31"));
        assert!(config
            .diffs
            .iter()
            .any(|d| d.key == "user.address.city" && d.dest_value == "New York"));
        assert!(config
            .diffs
            .iter()
            .any(|d| d.key == "user.address.zip" && d.source_value == "null"));
    }

    #[tokio::test]
    async fn test_array_of_primitives() {
        let source = r#"[1, 2, 3, 4]"#;
        let dest = r#"[1, 2, 5]"#;

        let source_value: Value = serde_json::from_str(source).unwrap();
        let dest_value: Value = serde_json::from_str(dest).unwrap();

        let result = json_diff("test".to_string(), source_value, dest_value)
            .await
            .unwrap();
        let config = result.unwrap();

        // No length diff
        assert!(!config.diffs.iter().any(|d| d.key == "length"));
        assert!(config
            .diffs
            .iter()
            .any(|d| d.key == "[2]" && d.source_value == "3" && d.dest_value == "5"));
        assert!(config
            .diffs
            .iter()
            .any(|d| d.key == "[3]" && d.source_value == "4" && d.dest_value == "null"));
    }

    #[tokio::test]
    async fn test_secrets_with_supabase_filter() {
        let source = r#"[
            {"name": "MY_SECRET", "updated_at": "2025-01-01T00:00:00Z", "value": "secret1"},
            {"name": "SUPABASE_URL", "updated_at": "2025-01-01T00:00:00Z", "value": "old_url"},
            {"name": "ANOTHER_SECRET", "updated_at": "2025-01-01T00:00:00Z", "value": "secret2"}
        ]"#;
        let dest = r#"[
            {"name": "MY_SECRET", "updated_at": "2025-01-02T00:00:00Z", "value": "secret1_new"},
            {"name": "SUPABASE_URL", "updated_at": "2025-01-02T00:00:00Z", "value": "new_url"},
            {"name": "SUPABASE_ANON_KEY", "updated_at": "2025-01-02T00:00:00Z", "value": "anon_key"}
        ]"#;

        let source_value: Value = serde_json::from_str(source).unwrap();
        let dest_value: Value = serde_json::from_str(dest).unwrap();

        let result = json_diff("Secrets".to_string(), source_value, dest_value)
            .await
            .unwrap();
        let config = result.unwrap();

        // After filtering SUPABASE_ secrets:
        // Source has: MY_SECRET, ANOTHER_SECRET
        // Dest has: MY_SECRET
        // So we should see:
        // - [0] changed (MY_SECRET value changed)
        // - [1] removed (ANOTHER_SECRET)
        assert_eq!(config.diffs.len(), 2);
        assert!(config.diffs.iter().any(|d| d.key == "[0]")); // MY_SECRET changed
        assert!(config
            .diffs
            .iter()
            .any(|d| d.key == "[1]" && d.source_value.contains("ANOTHER_SECRET"))); // ANOTHER_SECRET removed

        // Should not have any SUPABASE_ related diffs
        for diff in &config.diffs {
            assert!(!diff.source_value.contains("SUPABASE_"));
            assert!(!diff.dest_value.contains("SUPABASE_"));
        }
    }

    #[tokio::test]
    async fn test_array_object_diff_whole_object() {
        let source = r#"[
            {"name": "item1", "value": 100, "active": true}
        ]"#;
        let dest = r#"[
            {"name": "item1", "value": 200, "active": true}
        ]"#;

        let source_value: Value = serde_json::from_str(source).unwrap();
        let dest_value: Value = serde_json::from_str(dest).unwrap();

        let result = json_diff("test".to_string(), source_value, dest_value)
            .await
            .unwrap();
        let config = result.unwrap();

        // Should report the whole object as changed
        assert_eq!(config.diffs.len(), 1);
        assert!(config.diffs.iter().any(|d| d.key == "[0]"));
        assert!(config.diffs[0].source_value.contains("\"value\":100"));
        assert!(config.diffs[0].dest_value.contains("\"value\":200"));
    }
}

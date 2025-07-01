use crate::models::migrate::{ProjectConfig, DiffEntry};
use crate::models::AppState;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use tower_sessions::Session;

// Define the query parameters for the endpoint
#[derive(Debug, Deserialize)]
pub struct PreviewQuery {
    pub source_id: String,
    pub dest_id: String,
    pub auth: Option<bool>,
    pub postgrest: Option<bool>,
    pub edge_functions: Option<bool>,
    pub secrets: Option<bool>,
    pub postgres: Option<bool>,
}

// Define the response structure
#[derive(Debug, Serialize)]
pub struct PreviewResponse {
    pub configs: Vec<ProjectConfig>,
}

// Define error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// Custom error type for this endpoint
#[derive(Debug)]
pub enum PreviewError {
    Unauthorized,
    ApiError(String),
    JsonError(serde_json::Error),
    SessionError(String),
}

impl IntoResponse for PreviewError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message) = match self {
            PreviewError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            PreviewError::ApiError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            PreviewError::JsonError(err) => (StatusCode::BAD_REQUEST, format!("JSON error: {}", err)),
            PreviewError::SessionError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Session error: {}", msg)),
        };

        let body = Json(ErrorResponse {
            error: error_message,
        });

        (status, body).into_response()
    }
}

impl From<serde_json::Error> for PreviewError {
    fn from(err: serde_json::Error) -> Self {
        PreviewError::JsonError(err)
    }
}

pub async fn preview_handler(
    State(app_state): State<AppState>,
    Query(params): Query<PreviewQuery>,
    session: Session,
) -> Result<impl IntoResponse, PreviewError> {

    // TODO: Check authentication

    let mut project_config: Vec<ProjectConfig> = Vec::new();
    let mut config_json: Vec<(String, String, String)> = Vec::new();

    // Check Auth config
    if params.auth.unwrap_or(false) {
        let source_config = mgmt_api_get(&session, format!("/projects/{}/config/auth", params.source_id))
            .await
            .map_err(|e| PreviewError::ApiError(format!("Failed to get auth config: {:?}", e)))?;
        let dest_config = mgmt_api_get(&session,format!("/projects/{}/config/auth", params.dest_id))
            .await
            .map_err(|e| PreviewError::ApiError(format!("Failed to get auth config: {:?}", e)))?;
        config_json.push(("Auth".to_string(), source_config, dest_config));
    }

    // Check Postgrest config
    if params.postgrest.unwrap_or(false) {
        let source_config = mgmt_api_get(&session,format!("/projects/{}/postgrest", params.source_id))
            .await
            .map_err(|e| PreviewError::ApiError(format!("Failed to get postgrest config: {:?}", e)))?;
        let dest_config = mgmt_api_get(&session,format!("/projects/{}/postgrest", params.dest_id))
            .await
            .map_err(|e| PreviewError::ApiError(format!("Failed to get postgrest config: {:?}", e)))?;
        config_json.push(("Postgrest".to_string(), source_config, dest_config));
    }

    // Check Edge Functions config
    if params.edge_functions.unwrap_or(false) {
        let source_config = mgmt_api_get(&session,format!("/projects/{}/functions", params.source_id))
            .await
            .map_err(|e| PreviewError::ApiError(format!("Failed to get functions config: {:?}", e)))?;
        let dest_config = mgmt_api_get(&session,format!("/projects/{}/functions", params.dest_id))
            .await
            .map_err(|e| PreviewError::ApiError(format!("Failed to get functions config: {:?}", e)))?;
        config_json.push(("EdgeFunctions".to_string(), source_config, dest_config));
    }

    // Check Secrets config
    if params.secrets.unwrap_or(false) {
        let source_config = mgmt_api_get(&session,format!("/projects/{}/secrets", params.source_id))
            .await
            .map_err(|e| PreviewError::ApiError(format!("Failed to get secrets config: {:?}", e)))?;
        let dest_config = mgmt_api_get(&session,format!("/projects/{}/secrets", params.dest_id))
            .await
            .map_err(|e| PreviewError::ApiError(format!("Failed to get secrets config: {:?}", e)))?;
        config_json.push(("Secrets".to_string(), source_config, dest_config));
    }

    // Check Postgres config
    if params.postgres.unwrap_or(false) {
        let url = "/config/database/postgres".to_string();
        let source_config = mgmt_api_get(&session,format!("/projects/{}{}", params.source_id, url))
            .await
            .map_err(|e| PreviewError::ApiError(format!("Failed to get postgres config: {:?}", e)))?;
        let dest_config = mgmt_api_get(&session,format!("/projects/{}{}", params.dest_id, url))
            .await
            .map_err(|e| PreviewError::ApiError(format!("Failed to get postgres config: {:?}", e)))?;
        config_json.push(("Postgres".to_string(), source_config, dest_config));
    }

    // Process each config and generate diffs
    for (service, source_json, dest_json) in config_json {
        let source: Value = serde_json::from_str(&source_json)?;
        let dest: Value = serde_json::from_str(&dest_json)?;

        let project_config_entry = json_diff(service.clone(), source.clone(), dest).await?;

        if let Some(config_entry) = project_config_entry {
            project_config.push(config_entry);
        }

        // Store in session (optional - you might want to remove this if not needed)
        if let Err(e) = session.insert(&service, source_json).await {
            eprintln!("Failed to insert preview results into session: {:?}", e);
            // Don't fail the request for session errors, just log
        }
    }

    Ok(Json(PreviewResponse {
        configs: project_config,
    }))
}

pub async fn mgmt_api_get(session: &Session, url: String) -> Result<String, PreviewError> {
    use reqwest::header::{ACCEPT, AUTHORIZATION};
    
    let constructed_url = format!("https://api.supabase.com/v1{}", url);
    
    let token_option: Option<String> = session
        .get("supabase_access_token")
        .await
        .map_err(|e| PreviewError::SessionError(format!("Failed to get token from session: {:?}", e)))?;
    
    let token = token_option.ok_or_else(|| {
        PreviewError::Unauthorized
    })?;

    let client = reqwest::Client::new();
    let api_response = client
        .get(&constructed_url)
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| PreviewError::ApiError(format!("Request failed: {:?}", e)))?;

    if api_response.status().is_success() {
        api_response
            .text()
            .await
            .map_err(|e| PreviewError::ApiError(format!("Error reading response body as text: {:?}", e)))
    } else {
        let status_code = api_response.status().as_u16();
        let error_text = api_response
            .text()
            .await
            .unwrap_or_else(|e| format!("Error reading response body: {}", e));
        Err(PreviewError::ApiError(format!(
            "HTTP request failed with status {}: {}",
            status_code, error_text
        )))
    }
}


pub async fn json_diff(
    config_type: String,
    source_value: Value,
    dest_value: Value,
) -> Result<Option<ProjectConfig>, PreviewError> {
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
) -> Result<Vec<DiffEntry>, PreviewError> {
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
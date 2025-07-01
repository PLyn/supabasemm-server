use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectConfig {
    pub name: String,
    pub diffs: Vec<DiffEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiffEntry {
    pub key: String,
    pub source_value: String,
    pub dest_value: String,
}
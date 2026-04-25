use std::collections::HashMap;

use llamafile_builder_core::models::ConfigSchemaItem;

/// A flattened, UI-ready config entry derived from the schema.
#[derive(Debug, Clone)]
pub enum ConfigEntry {
    /// Section header (non-interactive, or toggleable).
    SectionHeader {
        label: String,
        description: String,
        toggle_key: Option<String>,
    },
    /// A dropdown with fixed options.
    Dropdown {
        key: String,
        label: String,
        description: String,
        options: Vec<String>,
        indent: usize,
    },
    /// A numeric slider.
    Slider {
        key: String,
        label: String,
        description: String,
        min: f64,
        max: f64,
        step: f64,
        indent: usize,
    },
    /// A boolean toggle.
    Bool {
        key: String,
        label: String,
        description: String,
        indent: usize,
    },
    /// A text input.
    Text {
        key: String,
        label: String,
        description: String,
        #[allow(dead_code)]
        value_type: String,
        indent: usize,
    },
}

impl ConfigEntry {
    pub fn key(&self) -> Option<&str> {
        match self {
            ConfigEntry::SectionHeader { toggle_key, .. } => toggle_key.as_deref(),
            ConfigEntry::Dropdown { key, .. } => Some(key),
            ConfigEntry::Slider { key, .. } => Some(key),
            ConfigEntry::Bool { key, .. } => Some(key),
            ConfigEntry::Text { key, .. } => Some(key),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            ConfigEntry::SectionHeader { label, .. } => label,
            ConfigEntry::Dropdown { label, .. } => label,
            ConfigEntry::Slider { label, .. } => label,
            ConfigEntry::Bool { label, .. } => label,
            ConfigEntry::Text { label, .. } => label,
        }
    }

    pub fn description(&self) -> &str {
        match self {
            ConfigEntry::SectionHeader { description, .. } => description,
            ConfigEntry::Dropdown { description, .. } => description,
            ConfigEntry::Slider { description, .. } => description,
            ConfigEntry::Bool { description, .. } => description,
            ConfigEntry::Text { description, .. } => description,
        }
    }

    pub fn indent(&self) -> usize {
        match self {
            ConfigEntry::SectionHeader { .. } => 0,
            ConfigEntry::Dropdown { indent, .. } => *indent,
            ConfigEntry::Slider { indent, .. } => *indent,
            ConfigEntry::Bool { indent, .. } => *indent,
            ConfigEntry::Text { indent, .. } => *indent,
        }
    }

    pub fn is_interactive(&self) -> bool {
        !matches!(self, ConfigEntry::SectionHeader { toggle_key: None, .. })
    }

    /// Format the current value for display.
    pub fn format_value(&self, values: &HashMap<String, serde_json::Value>, defaults: &HashMap<String, serde_json::Value>) -> String {
        let key = match self.key() {
            Some(k) => k,
            None => return String::new(),
        };
        let val = values.get(key).or_else(|| defaults.get(key));

        match self {
            ConfigEntry::SectionHeader { toggle_key: Some(_), .. } => {
                match val.and_then(|v| v.as_bool()) {
                    Some(true) => "[x]".to_string(),
                    Some(false) => "[ ]".to_string(),
                    None => "[x]".to_string(),
                }
            }
            ConfigEntry::Bool { .. } => {
                match val.and_then(|v| v.as_bool()) {
                    Some(true) => "[x]".to_string(),
                    Some(false) => "[ ]".to_string(),
                    None => "[ ]".to_string(),
                }
            }
            ConfigEntry::Dropdown {  .. } => {
                val.map(|v| {
                    let s = match v {
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::String(s) => s.clone(),
                        _ => v.to_string(),
                    };
                    s
                }).unwrap_or_default()
            }
            ConfigEntry::Slider { .. } => {
                val.and_then(|v| v.as_f64())
                    .map(|f| {
                        if f.fract() == 0.0 { format!("{}", f as i64) }
                        else { format!("{}", f) }
                    })
                    .unwrap_or_default()
            }
            ConfigEntry::Text { .. } => {
                val.map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    _ => v.to_string(),
                }).unwrap_or_default()
            }
            _ => String::new(),
        }
    }
}

/// Flatten the schema into a list of UI entries, skipping hidden items
/// and items under disabled toggleable sections.
pub fn flatten_schema(
    schema: &[ConfigSchemaItem],
    values: &HashMap<String, serde_json::Value>,
    defaults: &HashMap<String, serde_json::Value>,
) -> Vec<ConfigEntry> {
    let mut entries = Vec::new();
    flatten_items(schema, &mut entries, 0, values, defaults);
    entries
}

fn flatten_items(
    items: &[ConfigSchemaItem],
    entries: &mut Vec<ConfigEntry>,
    indent: usize,
    values: &HashMap<String, serde_json::Value>,
    defaults: &HashMap<String, serde_json::Value>,
) {
    for item in items {
        match item {
            ConfigSchemaItem::Section {
                label,
                key,
                description,
                default,
                items: nested,
                ..
            } => {
                entries.push(ConfigEntry::SectionHeader {
                    label: label.clone(),
                    description: description.as_deref().unwrap_or("").to_string(),
                    toggle_key: key.clone(),
                });
                // Only show nested items if section is enabled
                let section_enabled = if let Some(k) = key {
                    values.get(k)
                        .or_else(|| defaults.get(k))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(default.unwrap_or(true))
                } else {
                    true
                };
                if section_enabled {
                    flatten_items(nested, entries, indent + 1, values, defaults);
                }
            }
            ConfigSchemaItem::Dropdown {
                key,
                label,
                description,
                options,
                hidden,
                ..
            } => {
                if *hidden { continue; }
                let opts: Vec<String> = options.iter().map(|o| match o {
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::String(s) => s.clone(),
                    _ => o.to_string(),
                }).collect();
                entries.push(ConfigEntry::Dropdown {
                    key: key.clone(),
                    label: label.clone(),
                    description: description.clone(),
                    options: opts,
                    indent,
                });
            }
            ConfigSchemaItem::Slider {
                key,
                label,
                description,
                min,
                max,
                step,
                hidden,
                ..
            } => {
                if *hidden { continue; }
                entries.push(ConfigEntry::Slider {
                    key: key.clone(),
                    label: label.clone(),
                    description: description.clone(),
                    min: *min,
                    max: *max,
                    step: *step,
                    indent,
                });
            }
            ConfigSchemaItem::Bool {
                key,
                label,
                description,
                hidden,
                ..
            } => {
                if *hidden { continue; }
                entries.push(ConfigEntry::Bool {
                    key: key.clone(),
                    label: label.clone(),
                    description: description.clone(),
                    indent,
                });
            }
            ConfigSchemaItem::Text {
                key,
                label,
                description,
                value_type,
                hidden,
                ..
            } => {
                if *hidden { continue; }
                entries.push(ConfigEntry::Text {
                    key: key.clone(),
                    label: label.clone(),
                    description: description.clone(),
                    value_type: value_type.clone(),
                    indent,
                });
            }
        }
    }
}

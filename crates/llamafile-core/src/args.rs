use std::collections::HashMap;

use crate::models::*;

struct ArgsContext<'a> {
    config_values: &'a HashMap<String, serde_json::Value>,
    model_filename: &'a str,
    mmproj_filename: Option<&'a str>,
    has_mmproj: bool,
}

pub fn generate_args_from_schema(
    schema: &[ConfigSchemaItem],
    config_values: &HashMap<String, serde_json::Value>,
    model_filename: &str,
    mmproj_filename: Option<&str>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    let ctx = ArgsContext {
        config_values,
        model_filename,
        mmproj_filename,
        has_mmproj: mmproj_filename.is_some(),
    };
    process_schema_items(schema, &ctx, &mut lines);
    lines.push("...".to_string());
    lines.join("\n")
}

fn process_schema_items(items: &[ConfigSchemaItem], ctx: &ArgsContext, lines: &mut Vec<String>) {
    for item in items {
        match item {
            ConfigSchemaItem::Section {
                key,
                default,
                flag,
                items: nested_items,
                ..
            } => {
                if let Some(section_key) = key {
                    let enabled = ctx
                        .config_values
                        .get(section_key)
                        .and_then(|v| v.as_bool())
                        .unwrap_or(default.unwrap_or(true));
                    if !enabled {
                        continue;
                    }
                    if let Some(f) = flag {
                        lines.push(f.clone());
                    }
                }
                process_schema_items(nested_items, ctx, lines);
            }
            ConfigSchemaItem::Text {
                key,
                flag,
                default,
                conditional,
                ..
            } => {
                if let Some(cond) = conditional {
                    if cond == "has_mmproj" && !ctx.has_mmproj {
                        continue;
                    }
                }
                let flag = match flag {
                    Some(f) => f,
                    None => continue,
                };
                let value = ctx
                    .config_values
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| default.clone());
                let value_str = match &value {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    _ => continue,
                };
                if key == "threads" && value_str == "auto" {
                    continue;
                }
                if value_str.is_empty() {
                    continue;
                }
                let resolved = value_str
                    .replace("${MODEL_FILENAME}", ctx.model_filename)
                    .replace(
                        "${MMPROJ_FILENAME}",
                        ctx.mmproj_filename.unwrap_or(""),
                    );
                lines.push(flag.clone());
                lines.push(resolved);
            }
            ConfigSchemaItem::Dropdown {
                key,
                flag,
                default,
                conditional,
                ..
            } => {
                if let Some(cond) = conditional {
                    if cond == "has_mmproj" && !ctx.has_mmproj {
                        continue;
                    }
                }
                let flag = match flag {
                    Some(f) => f,
                    None => continue,
                };
                let value = ctx
                    .config_values
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| default.clone());
                let value_str = match &value {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    _ => continue,
                };
                lines.push(flag.clone());
                lines.push(value_str);
            }
            ConfigSchemaItem::Bool {
                key,
                flag,
                default,
                conditional,
                ..
            } => {
                if let Some(cond) = conditional {
                    if cond == "has_mmproj" && !ctx.has_mmproj {
                        continue;
                    }
                }
                let flag = match flag {
                    Some(f) => f,
                    None => continue,
                };
                let value = ctx
                    .config_values
                    .get(key)
                    .and_then(|v| v.as_bool())
                    .unwrap_or(*default);
                if value {
                    lines.push(flag.clone());
                }
            }
            ConfigSchemaItem::Slider {
                key,
                flag,
                default,
                conditional,
                ..
            } => {
                if let Some(cond) = conditional {
                    if cond == "has_mmproj" && !ctx.has_mmproj {
                        continue;
                    }
                }
                let flag = match flag {
                    Some(f) => f,
                    None => continue,
                };
                let value = ctx
                    .config_values
                    .get(key)
                    .and_then(|v| v.as_f64())
                    .unwrap_or(*default);
                let value_str = if value.fract() == 0.0 {
                    format!("{}", value as i64)
                } else {
                    format!("{}", value)
                };
                lines.push(flag.clone());
                lines.push(value_str);
            }
        }
    }
}

pub fn generate_system_prompt(agent: &AgentConfig) -> String {
    let mut prompt = agent.system_prompt.clone();
    let enabled_tools: Vec<_> = agent.tools.iter().filter(|t| t.enabled).collect();
    if !enabled_tools.is_empty() {
        prompt.push_str("\n\n## Available Tools\n\n");
        for tool in enabled_tools {
            prompt.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));
        }
    }
    prompt
}

pub fn estimate_size(model_size: u64, compute: &ComputeConfig, agent: &AgentConfig) -> u64 {
    let runtime_overhead: u64 = 5_000_000;
    let gpu_overhead: u64 = if compute.mode == ComputeMode::Gpu {
        let mut overhead = 0u64;
        if compute.cuda {
            overhead += 180_000_000;
        }
        if compute.rocm {
            overhead += 60_000_000;
        }
        if compute.metal {
            overhead += 10_000_000;
        }
        overhead
    } else {
        0
    };
    let agent_overhead: u64 = if agent.enabled { 1_000_000 } else { 0 };
    model_size + runtime_overhead + gpu_overhead + agent_overhead
}

/// Extract default config values from a schema. Useful for CLI defaults.
pub fn default_config_values(schema: &[ConfigSchemaItem]) -> HashMap<String, serde_json::Value> {
    let mut values = HashMap::new();
    collect_defaults(schema, &mut values);
    values
}

fn collect_defaults(items: &[ConfigSchemaItem], values: &mut HashMap<String, serde_json::Value>) {
    for item in items {
        match item {
            ConfigSchemaItem::Section {
                key,
                default,
                items: nested,
                ..
            } => {
                if let Some(k) = key {
                    values.insert(
                        k.clone(),
                        serde_json::Value::Bool(default.unwrap_or(false)),
                    );
                }
                collect_defaults(nested, values);
            }
            ConfigSchemaItem::Text { key, default, .. } => {
                values.insert(key.clone(), default.clone());
            }
            ConfigSchemaItem::Dropdown { key, default, .. } => {
                values.insert(key.clone(), default.clone());
            }
            ConfigSchemaItem::Bool { key, default, .. } => {
                values.insert(key.clone(), serde_json::Value::Bool(*default));
            }
            ConfigSchemaItem::Slider { key, default, .. } => {
                values.insert(
                    key.clone(),
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(*default)
                            .unwrap_or(serde_json::Number::from(0)),
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn load_schema() -> Vec<ConfigSchemaItem> {
        crate::config_manager::parse_config_data(include_str!("../../../data/config.json"))
            .expect("config.json must be valid")
    }

    // ---------- generate_args_from_schema ----------

    #[test]
    fn default_config_produces_expected_flags_without_mmproj() {
        let schema = load_schema();
        let values = default_config_values(&schema);
        let args = generate_args_from_schema(&schema, &values, "weights.gguf", None);
        let lines: Vec<&str> = args.lines().collect();

        assert!(lines.contains(&"-m"));
        assert!(lines.contains(&"/zip/weights.gguf"));
        assert!(lines.contains(&"--no-mmap"));

        assert!(!lines.contains(&"--mmproj"));

        assert!(lines.contains(&"--ctx-size"));
        assert!(lines.contains(&"4096"));
        assert!(lines.contains(&"--temp"));
        assert!(lines.contains(&"0.7"));

        assert!(lines.contains(&"--server"));
        assert!(lines.contains(&"--host"));
        assert!(lines.contains(&"127.0.0.1"));
        assert!(lines.contains(&"--port"));
        assert!(lines.contains(&"8080"));
        assert!(lines.contains(&"--webui"));

        assert_eq!(lines.last().copied(), Some("..."));
    }

    #[test]
    fn has_mmproj_includes_mmproj_flag() {
        let schema = load_schema();
        let values = default_config_values(&schema);
        let args = generate_args_from_schema(&schema, &values, "weights.gguf", Some("m.gguf"));
        let lines: Vec<&str> = args.lines().collect();
        assert!(lines.contains(&"--mmproj"));
        assert!(lines.contains(&"/zip/m.gguf"));
    }

    #[test]
    fn gpu_mode_disable_adds_gpu_flag() {
        let schema = load_schema();
        let mut values = default_config_values(&schema);
        values.insert("gpu_mode".to_string(), json!("DISABLE"));
        let args = generate_args_from_schema(&schema, &values, "weights.gguf", None);
        let lines: Vec<&str> = args.lines().collect();
        let gpu_idx = lines.iter().position(|l| *l == "--gpu").unwrap();
        assert_eq!(lines[gpu_idx + 1], "DISABLE");
    }

    #[test]
    fn empty_gpu_mode_does_not_emit_flag() {
        let schema = load_schema();
        let values = default_config_values(&schema);
        let args = generate_args_from_schema(&schema, &values, "w.gguf", None);
        let lines: Vec<&str> = args.lines().collect();
        assert!(!lines.contains(&"--gpu"));
    }

    #[test]
    fn server_mode_off_skips_server_and_nested_flags() {
        let schema = load_schema();
        let mut values = default_config_values(&schema);
        values.insert("server_enabled".to_string(), json!(false));
        let args = generate_args_from_schema(&schema, &values, "w.gguf", None);
        let lines: Vec<&str> = args.lines().collect();

        assert!(!lines.contains(&"--server"));
        assert!(!lines.contains(&"--host"));
        assert!(!lines.contains(&"--port"));
        assert!(!lines.contains(&"--webui"));
    }

    #[test]
    fn custom_context_length_is_respected() {
        let schema = load_schema();
        let mut values = default_config_values(&schema);
        values.insert("context_length".to_string(), json!(16384));
        let args = generate_args_from_schema(&schema, &values, "w.gguf", None);
        let lines: Vec<&str> = args.lines().collect();
        let ctx_idx = lines.iter().position(|l| *l == "--ctx-size").unwrap();
        assert_eq!(lines[ctx_idx + 1], "16384");
    }

    #[test]
    fn empty_text_value_is_skipped() {
        let schema = load_schema();
        let mut values = default_config_values(&schema);
        values.insert("host".to_string(), json!(""));
        let args = generate_args_from_schema(&schema, &values, "w.gguf", None);
        let lines: Vec<&str> = args.lines().collect();
        assert!(!lines.contains(&"--host"));
    }

    #[test]
    fn model_filename_substitution() {
        let schema = load_schema();
        let values = default_config_values(&schema);
        let args = generate_args_from_schema(&schema, &values, "MySpecialModel-Q4.gguf", None);
        assert!(args.contains("/zip/MySpecialModel-Q4.gguf"));
    }

    // ---------- generate_system_prompt ----------

    fn agent_with_tools(enabled: &[&str]) -> AgentConfig {
        let mut agent = AgentConfig::default();
        for tool in &mut agent.tools {
            tool.enabled = enabled.contains(&tool.id.as_str());
        }
        agent
    }

    #[test]
    fn system_prompt_without_tools_is_verbatim_prompt() {
        let agent = agent_with_tools(&[]);
        let prompt = generate_system_prompt(&agent);
        assert_eq!(prompt, agent.system_prompt);
        assert!(!prompt.contains("Available Tools"));
    }

    #[test]
    fn system_prompt_lists_only_enabled_tools() {
        let agent = agent_with_tools(&["shell", "python"]);
        let prompt = generate_system_prompt(&agent);
        assert!(prompt.contains("## Available Tools"));
        assert!(prompt.contains("**Shell Exec**"));
        assert!(prompt.contains("**Python REPL**"));
        assert!(!prompt.contains("**File I/O**"));
        assert!(!prompt.contains("**Web Fetch**"));
    }

    #[test]
    fn system_prompt_with_all_tools_enabled() {
        let agent = agent_with_tools(&[
            "shell",
            "file_io",
            "web_fetch",
            "python",
            "sqlite",
            "data_analysis",
        ]);
        let prompt = generate_system_prompt(&agent);
        for name in [
            "Shell Exec",
            "File I/O",
            "Web Fetch",
            "Python REPL",
            "SQLite",
            "Data Analysis",
        ] {
            assert!(prompt.contains(name), "missing {name}");
        }
    }

    // ---------- estimate_size ----------

    fn compute(mode: ComputeMode, cuda: bool, rocm: bool, metal: bool) -> ComputeConfig {
        ComputeConfig {
            mode,
            cuda,
            rocm,
            metal,
        }
    }

    #[test]
    fn estimate_size_cpu_only_is_model_plus_runtime() {
        let c = compute(ComputeMode::Cpu, false, false, false);
        let agent = AgentConfig {
            enabled: false,
            ..AgentConfig::default()
        };
        let size = estimate_size(1_000_000_000, &c, &agent);
        assert_eq!(size, 1_000_000_000 + 5_000_000);
    }

    #[test]
    fn estimate_size_gpu_cuda_adds_cuda_overhead() {
        let c = compute(ComputeMode::Gpu, true, false, false);
        let agent = AgentConfig {
            enabled: false,
            ..AgentConfig::default()
        };
        let size = estimate_size(1_000_000_000, &c, &agent);
        assert_eq!(size, 1_000_000_000 + 5_000_000 + 180_000_000);
    }

    #[test]
    fn estimate_size_gpu_all_flags_and_agent() {
        let c = compute(ComputeMode::Gpu, true, true, true);
        let agent = AgentConfig {
            enabled: true,
            ..AgentConfig::default()
        };
        let size = estimate_size(1_000_000_000, &c, &agent);
        assert_eq!(
            size,
            1_000_000_000 + 5_000_000 + 180_000_000 + 60_000_000 + 10_000_000 + 1_000_000
        );
    }

    #[test]
    fn estimate_size_cpu_mode_ignores_gpu_flags() {
        let c = compute(ComputeMode::Cpu, true, true, true);
        let agent = AgentConfig {
            enabled: false,
            ..AgentConfig::default()
        };
        let size = estimate_size(500_000_000, &c, &agent);
        assert_eq!(size, 500_000_000 + 5_000_000);
    }

    #[test]
    fn estimate_size_agent_overhead_is_flat() {
        let c = compute(ComputeMode::Cpu, false, false, false);
        let mut agent = AgentConfig::default();
        agent.enabled = true;
        let size = estimate_size(0, &c, &agent);
        assert_eq!(size, 5_000_000 + 1_000_000);
    }

    // ---------- default_config_values ----------

    #[test]
    fn default_config_values_includes_all_schema_keys() {
        let schema = load_schema();
        let values = default_config_values(&schema);
        assert!(values.contains_key("model_path"));
        assert!(values.contains_key("context_length"));
        assert!(values.contains_key("temperature"));
        assert!(values.contains_key("server_enabled"));
        assert!(values.contains_key("host"));
        assert!(values.contains_key("port"));
    }
}

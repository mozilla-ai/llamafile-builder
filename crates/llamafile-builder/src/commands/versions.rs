use llamafile_builder_core::config_manager::BuilderConfig;

pub fn run(config: &BuilderConfig, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&config.versions).unwrap()
        );
        return;
    }

    for group in &config.versions {
        println!("{}", group.name);
        if let Some(desc) = &group.description {
            println!("  {}", desc);
        }
        for release in &group.releases {
            let desc = release
                .description
                .as_deref()
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("");
            let desc_short = if desc.len() > 60 {
                format!("{}...", &desc[..57])
            } else {
                desc.to_string()
            };
            let mut suffix = String::new();
            if let Some(tag) = &release.tag {
                suffix.push_str(&format!(" [{}]", tag));
            }
            if release.default {
                suffix.push_str(" (default)");
            }
            println!(
                "  {} - {} ({}){}",
                release.version,
                release.release_date,
                llamafile_builder_core::models::format_size(release.size_bytes),
                suffix,
            );
            if !desc_short.is_empty() {
                println!("    {}", desc_short);
            }
        }
        println!();
    }
}

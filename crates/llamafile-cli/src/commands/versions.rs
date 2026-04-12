use llamafile_core::config_manager::BuilderConfig;

pub fn run(config: &BuilderConfig, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&config.versions).unwrap()
        );
        return;
    }

    for group in &config.versions {
        let tag = if group.tag.is_empty() {
            String::new()
        } else {
            format!(" ({})", group.tag)
        };
        println!("{}{}", group.name, tag);
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
            println!(
                "  {} - {} ({})",
                release.version,
                release.release_date,
                llamafile_core::models::format_size(release.size_bytes)
            );
            if !desc_short.is_empty() {
                println!("    {}", desc_short);
            }
        }
        println!();
    }
}

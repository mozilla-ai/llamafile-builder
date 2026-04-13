use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::app::*;

pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = if app.log_expanded {
        Layout::vertical([
            Constraint::Min(12),
            Constraint::Length(1), // status bar
            Constraint::Length(12),
        ])
        .split(frame.area())
    } else {
        Layout::vertical([
            Constraint::Min(12),
            Constraint::Length(1), // status bar
            Constraint::Length(3), // collapsed: border + 1 row + border
        ])
        .split(frame.area())
    };

    draw_dashboard(frame, app, chunks[0]);
    draw_status_bar(frame, app, chunks[1]);
    draw_log_pane(frame, app, chunks[2]);

    // Draw popup overlay if active
    match &app.popup {
        Popup::None => {}
        Popup::VersionSelect { selected } => draw_version_popup(frame, app, *selected),
        Popup::ModelSelect { search, selected } => draw_model_popup(frame, app, search, *selected),
        Popup::QuantSelect { role, options, selected } => {
            draw_quant_popup(frame, role, options, *selected)
        }
        Popup::OutputEdit { input } => draw_input_popup(frame, "Output Path", input),
        Popup::ConfigEdit { entries, selected, editing, edit_buf, dropdown_open, dropdown_selected } => {
            draw_config_popup_full(frame, app, entries, *selected, *editing, edit_buf, *dropdown_open, *dropdown_selected)
        }
        Popup::GpuEdit { specs, selected, adding, add_buf } => {
            draw_gpu_popup(frame, specs, *selected, *adding, add_buf)
        }
        Popup::ArgsEdit { textarea, .. } => {
            let area = centered_rect(72, 22, frame.area());
            frame.render_widget(Clear, area);
            frame.render_widget(&*textarea, area);
        }
        Popup::Confirm { message, .. } => draw_confirm_popup(frame, message),
    }
}

fn draw_dashboard(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered()
        .title(" llamafile builder ")
        .title_alignment(Alignment::Center);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let has_mmproj = app.has_mmproj();

    // Count rows: version, blank, model, weights, [mmproj], blank, output, blank, config, gpu, blank, status, buttons
    let mut constraints = vec![
        Constraint::Length(1), // version
        Constraint::Length(1), // blank
        Constraint::Length(1), // model
        Constraint::Length(1), // weights
    ];
    if has_mmproj {
        constraints.push(Constraint::Length(1)); // mmproj
    }
    constraints.extend_from_slice(&[
        Constraint::Length(1), // blank
        Constraint::Length(1), // output
        Constraint::Length(1), // blank
        Constraint::Length(1), // config
        Constraint::Length(1), // gpu
        Constraint::Length(1), // blank
        Constraint::Length(1), // status
        Constraint::Length(1), // download progress (or blank)
        Constraint::Min(1),   // buttons
    ]);

    let rows = Layout::vertical(constraints).split(inner);
    let mut row_idx = 0;

    let hl = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);

    // Helper to get style for a focus
    let style_for = |f: Focus| -> Style {
        if app.focus == f { hl } else { Style::default() }
    };

    // Version
    let marker = if app.focus == Focus::Version { ">" } else { " " };
    let label = format!(" {} Version    {}", marker, app.version_display());
    frame.render_widget(Paragraph::new(label).style(style_for(Focus::Version)), rows[row_idx]);
    row_idx += 1;

    // blank
    row_idx += 1;

    // Model
    let marker = if app.focus == Focus::Model { ">" } else { " " };
    let model_name = app.selected_model()
        .map(|m| format!("{} ({})", m.name, m.parameters))
        .unwrap_or_else(|| "(none)".to_string());
    let label = format!(" {} Model      {}", marker, model_name);
    frame.render_widget(Paragraph::new(label).style(style_for(Focus::Model)), rows[row_idx]);
    row_idx += 1;

    // Weights
    let marker = if app.focus == Focus::Weights { ">" } else { " " };
    let quant = app.quant_for_role("weights");
    let size = app.selected_model().and_then(|m| {
        m.files.iter().find(|f| f.role == "weights").and_then(|f| {
            let qname = app.quantizations.get("weights")
                .map(|s| s.as_str())
                .unwrap_or(f.quantizations.first().map(|q| q.name.as_str()).unwrap_or(""));
            f.quantizations.iter().find(|q| q.name == qname)
                .or(f.quantizations.first())
                .map(|q| llamafile_core::models::format_size(q.size_bytes))
        })
    }).unwrap_or_default();
    let label = format!(" {} Weights    {} ({})", marker, quant, size);
    frame.render_widget(Paragraph::new(label).style(style_for(Focus::Weights)), rows[row_idx]);
    row_idx += 1;

    // Mmproj (only if model has it)
    if has_mmproj {
        let marker = if app.focus == Focus::Mmproj { ">" } else { " " };
        if app.mmproj_disabled {
            let label = format!(" {} Mmproj     [ ] disabled (Space=enable)", marker);
            frame.render_widget(
                Paragraph::new(label).style(Style::default().fg(Color::DarkGray)),
                rows[row_idx],
            );
        } else {
            let quant = app.quant_for_role("mmproj");
            let label = format!(" {} Mmproj     [x] {}", marker, quant);
            frame.render_widget(Paragraph::new(label).style(style_for(Focus::Mmproj)), rows[row_idx]);
        }
        row_idx += 1;
    }

    // blank
    row_idx += 1;

    // Output (full path)
    let marker = if app.focus == Focus::Output { ">" } else { " " };
    let output = app.effective_output();
    let label = format!(" {} Output     {}", marker, output);
    frame.render_widget(Paragraph::new(label).style(style_for(Focus::Output)), rows[row_idx]);
    row_idx += 1;

    // blank
    row_idx += 1;

    // Config
    let config_style = if app.args_override.is_some() {
        Style::default().fg(Color::DarkGray)
    } else {
        style_for(Focus::Config)
    };
    let marker = if app.focus == Focus::Config { ">" } else { " " };
    let label = format!(" {} Config     {}", marker, app.config_summary());
    frame.render_widget(Paragraph::new(label).style(config_style), rows[row_idx]);
    row_idx += 1;

    // GPU
    let marker = if app.focus == Focus::Gpu { ">" } else { " " };
    let gpu = if app.gpu_libs.is_empty() {
        "(none)".to_string()
    } else {
        app.gpu_libs.join(", ")
    };
    let label = format!(" {} GPU        {}", marker, gpu);
    frame.render_widget(Paragraph::new(label).style(style_for(Focus::Gpu)), rows[row_idx]);
    row_idx += 1;

    // blank
    row_idx += 1;

    // Build status
    let status_style = match &app.build_status {
        BuildStatus::Idle => {
            if app.can_build() { Style::default().fg(Color::Green) }
            else { Style::default().fg(Color::Red) }
        }
        BuildStatus::Building { .. } => Style::default().fg(Color::Yellow),
        BuildStatus::Complete { .. } => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        BuildStatus::Failed { .. } => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    };
    let status_text = match &app.build_status {
        BuildStatus::Idle => {
            if app.can_build() {
                "   Ready to build".to_string()
            } else {
                let errors = app.validation_errors();
                format!("   {}", errors.first().unwrap_or(&"Not ready".to_string()))
            }
        }
        BuildStatus::Building { phase, progress } => {
            let bar_width = 20;
            let filled = (progress * bar_width as f32) as usize;
            let empty = bar_width - filled;
            format!(
                "   [{}{}] {:.0}% {}",
                "█".repeat(filled),
                "░".repeat(empty),
                progress * 100.0,
                phase
            )
        }
        BuildStatus::Complete { path, size } => {
            format!("   Done: {} ({})", path, llamafile_core::models::format_size(*size))
        }
        BuildStatus::Failed { error } => {
            format!("   Failed: {}", error)
        }
    };
    frame.render_widget(Paragraph::new(status_text).style(status_style), rows[row_idx]);
    row_idx += 1;

    // Download progress (below build status)
    if let Some(ref dl) = app.download_info {
        let bar_width = 20;
        let filled = (dl.percentage / 100.0 * bar_width as f32) as usize;
        let empty = bar_width - filled;
        let dl_text = format!(
            "   [{}{}] {:.0}% {} ({}/{})",
            "█".repeat(filled),
            "░".repeat(empty),
            dl.percentage,
            dl.filename,
            llamafile_core::models::format_size(dl.bytes_downloaded),
            llamafile_core::models::format_size(dl.bytes_total),
        );
        frame.render_widget(
            Paragraph::new(dl_text).style(Style::default().fg(Color::Cyan)),
            rows[row_idx],
        );
    }
    row_idx += 1;

    // Buttons row
    let build_style = if app.focus == Focus::Build { hl } else { Style::default() };
    let args_style = if app.focus == Focus::ShowArgs { hl } else { Style::default() };
    let quit_style = if app.focus == Focus::Quit { hl } else { Style::default() };

    let buttons = Line::from(vec![
        Span::raw("   "),
        Span::styled("[ Build ]", build_style),
        Span::raw("   "),
        Span::styled("[ Show .args ]", args_style),
        Span::raw("   "),
        Span::styled("[ Quit ]", quit_style),
    ]);
    frame.render_widget(Paragraph::new(buttons), rows[row_idx]);
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let desc = app.focus.description();
    let bg = Style::default().fg(Color::DarkGray).bg(Color::DarkGray);
    let key_style = Style::default().fg(Color::White).bg(Color::DarkGray).add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(Color::Gray).bg(Color::DarkGray);

    let keys: Vec<Span> = match app.focus {
        Focus::Build => vec![
            Span::styled("Enter", key_style), Span::styled("=build", text_style),
        ],
        Focus::ShowArgs => vec![
            Span::styled("Enter", key_style), Span::styled("=preview", text_style),
        ],
        Focus::Quit => vec![
            Span::styled("Enter", key_style), Span::styled("=quit", text_style),
        ],
        Focus::Mmproj => vec![
            Span::styled("Space", key_style), Span::styled("=toggle  ", text_style),
            Span::styled("Enter", key_style), Span::styled("=change  ", text_style),
            Span::styled("↑↓", key_style), Span::styled("=navigate", text_style),
        ],
        _ => vec![
            Span::styled("Enter", key_style), Span::styled("=change  ", text_style),
            Span::styled("↑↓", key_style), Span::styled("=navigate", text_style),
        ],
    };

    let mut spans = vec![
        Span::styled(format!(" {}  │  ", desc), text_style),
    ];
    spans.extend(keys);
    spans.extend(vec![
        Span::styled("  │  ", text_style),
        Span::styled("l", key_style), Span::styled("=logs  ", text_style),
        Span::styled("q", key_style), Span::styled("=quit", text_style),
    ]);

    frame.render_widget(Paragraph::new(Line::from(spans)).style(bg), area);
}

fn draw_log_pane(frame: &mut Frame, app: &App, area: Rect) {
    let filtered = app.filtered_logs();
    let count = filtered.len();

    if app.log_expanded {
        let level_nav = match app.log_level {
            crate::app::LogLevel::Error => "< ERROR".to_string(),
            crate::app::LogLevel::Debug => "DEBUG >".to_string(),
            _ => format!("< {} >", app.log_level.as_str()),
        };
        let block = Block::bordered().title(Line::from(vec![
            Span::raw(" Logs "),
            Span::styled(level_nav, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::raw(format!(" ({}/{}) ", count, app.logs.len())),
            Span::styled("l", Style::default().fg(Color::White)),
            Span::raw("=collapse "),
            Span::styled("[", Style::default().fg(Color::White)),
            Span::styled("]", Style::default().fg(Color::White)),
            Span::raw("=scroll "),
        ]));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Build the full text with colors
        let lines: Vec<Line> = filtered
            .iter()
            .map(|entry| {
                let style = match entry.level {
                    crate::app::LogLevel::Error => Style::default().fg(Color::Red),
                    crate::app::LogLevel::Warn => Style::default().fg(Color::Yellow),
                    crate::app::LogLevel::Info => Style::default(),
                    crate::app::LogLevel::Debug => Style::default().fg(Color::DarkGray),
                };
                Line::styled(entry.message.as_str(), style)
            })
            .collect();

        // Scroll from bottom: total lines - visible height - offset
        let total_lines = lines.len() as u16;
        let visible_height = inner.height;
        let max_scroll = total_lines.saturating_sub(visible_height);
        // Clamp offset so it can't exceed scrollable range
        let clamped_offset = app.log_scroll_offset.min(max_scroll);
        let scroll_y = max_scroll.saturating_sub(clamped_offset);

        let paragraph = Paragraph::new(lines).scroll((scroll_y, 0));
        frame.render_widget(paragraph, inner);
    } else {
        let level_nav = match app.log_level {
            crate::app::LogLevel::Error => "< ERROR".to_string(),
            crate::app::LogLevel::Debug => "DEBUG >".to_string(),
            _ => format!("< {} >", app.log_level.as_str()),
        };
        let block = Block::bordered().title(Line::from(vec![
            Span::raw(" Logs "),
            Span::styled(level_nav, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled("l", Style::default().fg(Color::White)),
            Span::raw("=expand "),
        ]));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let last = filtered.last().map(|e| e.message.as_str()).unwrap_or("");
        frame.render_widget(
            Paragraph::new(format!(" {}", last)).style(Style::default().fg(Color::DarkGray)),
            inner,
        );
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

fn draw_version_popup(frame: &mut Frame, app: &App, selected: usize) {
    let area = centered_rect(60, 14, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(" Select Version ")
        .title_bottom(" ↑↓=navigate  Enter=select  Esc=cancel ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut items = Vec::new();
    for group in &app.config.versions {
        for release in &group.releases {
            let tag = if group.default { " (default)" } else { "" };
            let label = format!("{} / {} [{}]{}", group.name, release.version, group.tag, tag);
            items.push(label);
        }
    }

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let style = if i == selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!("  {}", label)).style(style)
        })
        .collect();
    frame.render_widget(List::new(list_items), inner);
}

fn draw_model_popup(frame: &mut Frame, app: &App, search: &str, selected: usize) {
    let area = centered_rect(72, 22, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(" Select Model ")
        .title_bottom(" Type to search  ↑↓=navigate  Enter=select  Esc=cancel ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1), // search
        Constraint::Length(1), // blank
        Constraint::Min(1),   // list
    ])
    .split(inner);

    let search_line = format!(" Search: {}_", search);
    frame.render_widget(
        Paragraph::new(search_line).style(Style::default().fg(Color::Cyan)),
        rows[0],
    );

    let filtered = llamafile_core::search_models(
        &app.config.models,
        Some(search).filter(|s| !s.is_empty()),
        None,
    );

    let list_items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, model)| {
            let size: u64 = model.files.iter()
                .flat_map(|f| f.quantizations.first())
                .map(|q| q.size_bytes)
                .sum();
            let tags = model.tags.join(", ");
            let label = format!(
                "  {} — {} {} [{}]",
                model.name,
                model.parameters,
                llamafile_core::models::format_size(size),
                tags
            );
            let style = if i == selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(label).style(style)
        })
        .collect();
    frame.render_widget(List::new(list_items), rows[2]);
}

fn draw_quant_popup(frame: &mut Frame, role: &str, options: &[String], selected: usize) {
    let height = (options.len() + 4).min(12) as u16;
    let area = centered_rect(45, height, frame.area());
    frame.render_widget(Clear, area);
    let title = format!(" Select {} Quantization ", role);
    let block = Block::bordered()
        .title(title)
        .title_bottom(" ↑↓=navigate  Enter=select  Esc=cancel ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let list_items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let style = if i == selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!("  {}", name)).style(style)
        })
        .collect();
    frame.render_widget(List::new(list_items), inner);
}

fn draw_input_popup(frame: &mut Frame, title: &str, input: &str) {
    let area = centered_rect(70, 5, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(format!(" {} ", title))
        .title_bottom(" Type path  Enter=confirm  Esc=cancel ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = format!(" {}_", input);
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::Cyan)),
        inner,
    );
}

fn draw_config_popup_full(
    frame: &mut Frame,
    app: &App,
    entries: &[crate::config_ui::ConfigEntry],
    selected: usize,
    editing: bool,
    edit_buf: &str,
    dropdown_open: bool,
    dropdown_selected: usize,
) {
    use crate::config_ui::ConfigEntry;

    let defaults = llamafile_core::args::default_config_values(&app.config.config_schema);

    let height = (entries.len() + 5).min(22) as u16;
    let area = centered_rect(68, height, frame.area());
    frame.render_widget(Clear, area);

    let desc = entries.get(selected).map(|e| e.description()).unwrap_or("");
    let help = if dropdown_open {
        " ↑↓=navigate  Enter=select  Esc=cancel "
    } else if editing {
        " Type value  Enter=save  Esc=cancel "
    } else if entries.get(selected).map(|e| matches!(e, ConfigEntry::Slider { .. })).unwrap_or(false) {
        " ←→=adjust  Enter=type value  Esc=close "
    } else {
        " ↑↓=navigate  Enter/Space=toggle/edit  Esc=close "
    };
    let block = Block::bordered()
        .title(" Configuration ")
        .title_bottom(help);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1), // description
        Constraint::Length(1), // blank
        Constraint::Min(1),   // entries
    ]).split(inner);

    frame.render_widget(
        Paragraph::new(format!(" {}", desc)).style(Style::default().fg(Color::Cyan)),
        rows[0],
    );

    let list_items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_selected = i == selected;
            let indent = "  ".repeat(entry.indent());
            let value = entry.format_value(&app.config_values, &defaults);
            let marker = if is_selected && entry.is_interactive() { ">" } else { " " };

            let line = match entry {
                ConfigEntry::SectionHeader { label, toggle_key: Some(_), .. } => {
                    format!("{} {} ── {} ──", marker, value, label)
                }
                ConfigEntry::SectionHeader { label, toggle_key: None, .. } => {
                    format!("      ── {} ──", label)
                }
                ConfigEntry::Bool { label, .. } => {
                    format!("{}  {} {} {}", marker, indent, value, label)
                }
                _ if editing && is_selected => {
                    format!("{}  {} {}: {}_", marker, indent, entry.label(), edit_buf)
                }
                _ => {
                    format!("{}  {} {}: {}", marker, indent, entry.label(), value)
                }
            };

            let style = if is_selected && entry.is_interactive() {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if matches!(entry, ConfigEntry::SectionHeader { .. }) {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();
    frame.render_widget(List::new(list_items), rows[2]);

    // Draw dropdown overlay if open
    if dropdown_open {
        if let Some(ConfigEntry::Dropdown { options, .. }) = entries.get(selected) {
            let dd_height = (options.len() + 2).min(10) as u16;
            let dd_area = centered_rect(30, dd_height, frame.area());
            frame.render_widget(Clear, dd_area);
            let dd_block = Block::bordered().title(" Select ");
            let dd_inner = dd_block.inner(dd_area);
            frame.render_widget(dd_block, dd_area);

            let dd_items: Vec<ListItem> = options
                .iter()
                .enumerate()
                .map(|(i, opt)| {
                    let style = if i == dropdown_selected {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    ListItem::new(format!("  {}", opt)).style(style)
                })
                .collect();
            frame.render_widget(List::new(dd_items), dd_inner);
        }
    }
}

fn draw_gpu_popup(
    frame: &mut Frame,
    specs: &[String],
    selected: usize,
    adding: bool,
    add_buf: &str,
) {
    let height = (specs.len() + 6).min(14) as u16;
    let area = centered_rect(55, height, frame.area());
    frame.render_widget(Clear, area);
    let help = if adding {
        " Type spec (cuda, rocm:linux)  Enter=add  Esc=cancel "
    } else {
        " a=add  d=delete  ↑↓=navigate  Esc=close "
    };
    let block = Block::bordered()
        .title(" GPU Libraries ")
        .title_bottom(help);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(if adding { 1 } else { 0 }),
    ])
    .split(inner);

    if specs.is_empty() && !adding {
        frame.render_widget(
            Paragraph::new("  (none) — press 'a' to add")
                .style(Style::default().fg(Color::DarkGray)),
            rows[0],
        );
    } else {
        let items: Vec<ListItem> = specs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let style = if i == selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(format!("  {}", s)).style(style)
            })
            .collect();
        frame.render_widget(List::new(items), rows[0]);
    }

    if adding {
        let text = format!(" Add: {}_", add_buf);
        frame.render_widget(
            Paragraph::new(text).style(Style::default().fg(Color::Cyan)),
            rows[1],
        );
    }
}


fn draw_confirm_popup(frame: &mut Frame, message: &str) {
    let area = centered_rect(50, 5, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(" Confirm ")
        .title_bottom(" y=yes  n=no ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(format!(" {}", message)), inner);
}

use crossterm::event::{self, KeyCode, KeyModifiers};

use crate::app::*;

/// Handle a key event. Returns true if the app should redraw.
pub fn handle_key(app: &mut App, key: event::KeyEvent) -> bool {
    // Global keys
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return true;
    }

    // Log pane toggle
    if matches!(key.code, KeyCode::Char('l')) && matches!(app.popup, Popup::None) {
        app.log_expanded = !app.log_expanded;
        return true;
    }

    // Log controls (only when no popup is open)
    if matches!(app.popup, Popup::None) {
        match key.code {
            KeyCode::Char('+') => {
                app.log_level = app.log_level.lower();
                app.log_scroll_offset = 0;
                return true;
            }
            KeyCode::Char('-') => {
                app.log_level = app.log_level.raise();
                app.log_scroll_offset = 0;
                return true;
            }
            KeyCode::Char('[') if app.log_expanded => {
                let max = app.filtered_logs().len() as u16;
                app.log_scroll_offset = (app.log_scroll_offset + 1).min(max.saturating_sub(1));
                return true;
            }
            KeyCode::Char(']') if app.log_expanded => {
                app.log_scroll_offset = app.log_scroll_offset.saturating_sub(1);
                return true;
            }
            _ => {}
        }
    }

    // Popup handling
    match &app.popup {
        Popup::None => handle_dashboard_key(app, key),
        Popup::VersionSelect { .. } => handle_version_popup(app, key),
        Popup::ModelSelect { .. } => handle_model_popup(app, key),
        Popup::QuantSelect { .. } => handle_quant_popup(app, key),
        Popup::OutputEdit { .. } => handle_input_popup(app, key),
        Popup::ConfigEdit { .. } => handle_config_popup(app, key),
        Popup::GpuEdit { .. } => handle_gpu_popup(app, key),
        Popup::ArgsPreview { .. } => handle_args_popup(app, key),
        Popup::Confirm { .. } => handle_confirm_popup(app, key),
    }
}

fn handle_dashboard_key(app: &mut App, key: event::KeyEvent) -> bool {
    let has_mmproj = app.has_mmproj();
    match key.code {
        KeyCode::Down | KeyCode::Tab => {
            app.focus = app.focus.next(has_mmproj);
            true
        }
        KeyCode::Up | KeyCode::BackTab => {
            app.focus = app.focus.prev(has_mmproj);
            true
        }
        KeyCode::Char('q') => {
            if matches!(app.build_status, BuildStatus::Building { .. }) {
                app.popup = Popup::Confirm {
                    message: "Build in progress. Quit anyway?".to_string(),
                    action: ConfirmAction::Quit,
                };
            } else {
                app.should_quit = true;
            }
            true
        }
        KeyCode::Char(' ') if app.focus == Focus::Mmproj => {
            app.mmproj_disabled = !app.mmproj_disabled;
            true
        }
        KeyCode::Enter => {
            match app.focus {
                Focus::Version => {
                    app.popup = Popup::VersionSelect { selected: 0 };
                }
                Focus::Model => {
                    app.popup = Popup::ModelSelect {
                        search: String::new(),
                        selected: 0,
                    };
                }
                Focus::Weights => {
                    if let Some(model) = app.selected_model() {
                        if let Some(file) = model.files.iter().find(|f| f.role == "weights") {
                            let options: Vec<String> =
                                file.quantizations.iter().map(|q| q.name.clone()).collect();
                            if !options.is_empty() {
                                app.popup = Popup::QuantSelect {
                                    role: "weights".to_string(),
                                    options,
                                    selected: 0,
                                };
                            }
                        }
                    }
                }
                Focus::Mmproj => {
                    if !app.mmproj_disabled {
                        if let Some(model) = app.selected_model() {
                            if let Some(file) = model.files.iter().find(|f| f.role == "mmproj") {
                                let options: Vec<String> =
                                    file.quantizations.iter().map(|q| q.name.clone()).collect();
                                if !options.is_empty() {
                                    app.popup = Popup::QuantSelect {
                                        role: "mmproj".to_string(),
                                        options,
                                        selected: 0,
                                    };
                                }
                            }
                        }
                    }
                }
                Focus::Output => {
                    let current = app.effective_output();
                    app.popup = Popup::OutputEdit { input: current };
                }
                Focus::Config => {
                    open_config_popup(app);
                }
                Focus::Gpu => {
                    app.popup = Popup::GpuEdit {
                        specs: app.gpu_libs.clone(),
                        selected: 0,
                        adding: false,
                        add_buf: String::new(),
                    };
                }
                Focus::Build => {
                    if app.can_build() && !matches!(app.build_status, BuildStatus::Building { .. }) {
                        // Build will be triggered in the main loop
                        app.build_status = BuildStatus::Building {
                            phase: "starting".to_string(),
                            progress: 0.0,
                        };
                    }
                }
                Focus::ShowArgs => {
                    let content = if let Some(ref ov) = app.args_override {
                        ov.clone()
                    } else if let Ok(builder) = app.make_builder() {
                        builder.resolved_args()
                    } else {
                        "(error generating .args)".to_string()
                    };
                    app.popup = Popup::ArgsPreview { content, editing: false, cursor_line: 0 };
                }
                Focus::Quit => {
                    app.should_quit = true;
                }
            }
            true
        }
        _ => false,
    }
}

fn handle_version_popup(app: &mut App, key: event::KeyEvent) -> bool {
    let mut flat: Vec<(String, String)> = Vec::new();
    for group in &app.config.versions {
        for release in &group.releases {
            flat.push((group.name.clone(), release.version.clone()));
        }
    }

    if let Popup::VersionSelect { ref mut selected } = app.popup {
        match key.code {
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Down => *selected = (*selected + 1).min(flat.len().saturating_sub(1)),
            KeyCode::Enter => {
                if let Some((group, version)) = flat.get(*selected) {
                    app.version_group = Some(group.clone());
                    app.version = Some(version.clone());
                    app.custom_runtime = None;
                }
                app.popup = Popup::None;
            }
            KeyCode::Esc => app.popup = Popup::None,
            _ => {}
        }
    }
    true
}

fn handle_model_popup(app: &mut App, key: event::KeyEvent) -> bool {
    if let Popup::ModelSelect {
        ref mut search,
        ref mut selected,
    } = app.popup
    {
        let filtered = llamafile_core::search_models(
            &app.config.models,
            Some(search.as_str()).filter(|s| !s.is_empty()),
            None,
        );
        let count = filtered.len();

        match key.code {
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Down => *selected = (*selected + 1).min(count.saturating_sub(1)),
            KeyCode::Enter => {
                let filtered = llamafile_core::search_models(
                    &app.config.models,
                    Some(search.as_str()).filter(|s| !s.is_empty()),
                    None,
                );
                if let Some(model) = filtered.get(*selected) {
                    app.model_id = Some(model.id.clone());
                    app.custom_model_file = None;
                    app.quantizations.clear();
                }
                app.popup = Popup::None;
            }
            KeyCode::Esc => app.popup = Popup::None,
            KeyCode::Backspace => {
                search.pop();
                *selected = 0;
            }
            KeyCode::Char(c) => {
                search.push(c);
                *selected = 0;
            }
            _ => {}
        }
    }
    true
}

fn handle_quant_popup(app: &mut App, key: event::KeyEvent) -> bool {
    if let Popup::QuantSelect {
        ref role,
        ref options,
        ref mut selected,
    } = app.popup
    {
        match key.code {
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Down => *selected = (*selected + 1).min(options.len().saturating_sub(1)),
            KeyCode::Enter => {
                if let Some(name) = options.get(*selected) {
                    app.quantizations.insert(role.clone(), name.clone());
                }
                app.popup = Popup::None;
            }
            KeyCode::Esc => app.popup = Popup::None,
            _ => {}
        }
    }
    true
}

fn handle_input_popup(app: &mut App, key: event::KeyEvent) -> bool {
    if let Popup::OutputEdit { ref mut input } = app.popup {
        match key.code {
            KeyCode::Enter => {
                app.output_path = Some(input.clone());
                app.popup = Popup::None;
            }
            KeyCode::Esc => app.popup = Popup::None,
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char(c) => input.push(c),
            _ => {}
        }
    }
    true
}

fn handle_config_popup(app: &mut App, key: event::KeyEvent) -> bool {
    use crate::config_ui::ConfigEntry;

    if let Popup::ConfigEdit {
        ref entries,
        ref mut selected,
        ref mut editing,
        ref mut edit_buf,
        ref mut dropdown_open,
        ref mut dropdown_selected,
    } = app.popup
    {
        let entries_clone = entries.clone();
        let entry = entries_clone.get(*selected);

        if *dropdown_open {
            // Dropdown selection mode
            if let Some(ConfigEntry::Dropdown { key: k, options, .. }) = entry {
                match key.code {
                    KeyCode::Up => *dropdown_selected = dropdown_selected.saturating_sub(1),
                    KeyCode::Down => *dropdown_selected = (*dropdown_selected + 1).min(options.len().saturating_sub(1)),
                    KeyCode::Enter => {
                        if let Some(opt) = options.get(*dropdown_selected) {
                            let val: serde_json::Value = serde_json::from_str(opt)
                                .unwrap_or_else(|_| serde_json::Value::String(opt.clone()));
                            app.config_values.insert(k.clone(), val);
                        }
                        *dropdown_open = false;
                    }
                    KeyCode::Esc => *dropdown_open = false,
                    _ => {}
                }
            }
        } else if *editing {
            // Text/number editing mode
            match key.code {
                KeyCode::Enter => {
                    if let Some(entry) = entry {
                        if let Some(k) = entry.key() {
                            let parsed: serde_json::Value = serde_json::from_str(edit_buf)
                                .unwrap_or_else(|_| serde_json::Value::String(edit_buf.clone()));
                            app.config_values.insert(k.to_string(), parsed);
                        }
                    }
                    *editing = false;
                    edit_buf.clear();
                }
                KeyCode::Esc => {
                    *editing = false;
                    edit_buf.clear();
                }
                KeyCode::Backspace => { edit_buf.pop(); }
                KeyCode::Char(c) => edit_buf.push(c),
                _ => {}
            }
        } else {
            // Navigation mode
            match key.code {
                KeyCode::Up => {
                    // Skip non-interactive entries going up
                    let mut idx = selected.saturating_sub(1);
                    while idx > 0 && !entries_clone.get(idx).map(|e| e.is_interactive()).unwrap_or(false) {
                        idx -= 1;
                    }
                    if entries_clone.get(idx).map(|e| e.is_interactive()).unwrap_or(false) {
                        *selected = idx;
                    }
                }
                KeyCode::Down => {
                    let mut idx = *selected + 1;
                    while idx < entries_clone.len() && !entries_clone.get(idx).map(|e| e.is_interactive()).unwrap_or(false) {
                        idx += 1;
                    }
                    if idx < entries_clone.len() {
                        *selected = idx;
                    }
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if let Some(entry) = entry {
                        let defaults = llamafile_core::args::default_config_values(&app.config.config_schema);
                        let mut need_rebuild = false;
                        match entry {
                            ConfigEntry::SectionHeader { toggle_key: Some(k), .. } => {
                                let current = app.config_values.get(k)
                                    .or_else(|| defaults.get(k))
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(true);
                                app.config_values.insert(k.clone(), serde_json::Value::Bool(!current));
                                need_rebuild = true; // section expanded/collapsed
                            }
                            ConfigEntry::Bool { key: k, .. } => {
                                let current = app.config_values.get(k)
                                    .or_else(|| defaults.get(k))
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                app.config_values.insert(k.clone(), serde_json::Value::Bool(!current));
                            }
                            ConfigEntry::Dropdown { .. } => {
                                if let ConfigEntry::Dropdown { ref options, .. } = entry {
                                    let current = entry.format_value(&app.config_values, &defaults);
                                    let idx = options.iter().position(|o| *o == current).unwrap_or(0);
                                    *dropdown_selected = idx;
                                    *dropdown_open = true;
                                }
                            }
                            ConfigEntry::Slider { .. } => {
                                let current = entry.format_value(&app.config_values, &defaults);
                                *edit_buf = current;
                                *editing = true;
                            }
                            ConfigEntry::Text { .. } => {
                                let current = entry.format_value(&app.config_values, &defaults);
                                *edit_buf = current;
                                *editing = true;
                            }
                            _ => {}
                        }
                        if need_rebuild {
                            // Rebuild entries to show/hide section contents
                            let sel_label = entries_clone.get(*selected).map(|e| e.label().to_string());
                            rebuild_config_entries(app);
                            // Try to keep selection on the same entry
                            if let Popup::ConfigEdit { ref entries, ref mut selected, .. } = app.popup {
                                if let Some(label) = sel_label {
                                    if let Some(idx) = entries.iter().position(|e| e.label() == label) {
                                        *selected = idx;
                                    }
                                }
                            }
                        }
                    }
                }
                KeyCode::Left => {
                    // Decrease slider value
                    if let Some(ConfigEntry::Slider { key: k, min, step, .. }) = entry {
                        let defaults = llamafile_core::args::default_config_values(&app.config.config_schema);
                        let current = app.config_values.get(k)
                            .or_else(|| defaults.get(k))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(*min);
                        let new_val = (current - step).max(*min);
                        let val = serde_json::Number::from_f64(new_val)
                            .unwrap_or(serde_json::Number::from(0));
                        app.config_values.insert(k.clone(), serde_json::Value::Number(val));
                    }
                }
                KeyCode::Right => {
                    // Increase slider value
                    if let Some(ConfigEntry::Slider { key: k, max, step, .. }) = entry {
                        let defaults = llamafile_core::args::default_config_values(&app.config.config_schema);
                        let current = app.config_values.get(k)
                            .or_else(|| defaults.get(k))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let new_val = (current + step).min(*max);
                        let val = serde_json::Number::from_f64(new_val)
                            .unwrap_or(serde_json::Number::from(0));
                        app.config_values.insert(k.clone(), serde_json::Value::Number(val));
                    }
                }
                KeyCode::Esc => app.popup = Popup::None,
                _ => {}
            }
        }
    }
    true
}

fn handle_gpu_popup(app: &mut App, key: event::KeyEvent) -> bool {
    if let Popup::GpuEdit {
        ref mut specs,
        ref mut selected,
        ref mut adding,
        ref mut add_buf,
    } = app.popup
    {
        if *adding {
            match key.code {
                KeyCode::Enter => {
                    if !add_buf.is_empty() {
                        // Validate
                        if crate::parse_gpu_spec(add_buf).is_ok() {
                            if !specs.contains(add_buf) {
                                specs.push(add_buf.clone());
                            }
                        }
                    }
                    *adding = false;
                    add_buf.clear();
                }
                KeyCode::Esc => {
                    *adding = false;
                    add_buf.clear();
                }
                KeyCode::Backspace => {
                    add_buf.pop();
                }
                KeyCode::Char(c) => add_buf.push(c),
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Up => *selected = selected.saturating_sub(1),
                KeyCode::Down => {
                    *selected = (*selected + 1).min(specs.len().saturating_sub(1))
                }
                KeyCode::Char('a') => *adding = true,
                KeyCode::Char('d') | KeyCode::Delete => {
                    if *selected < specs.len() {
                        specs.remove(*selected);
                        if *selected > 0 {
                            *selected -= 1;
                        }
                    }
                }
                KeyCode::Esc => {
                    app.gpu_libs = specs.clone();
                    app.popup = Popup::None;
                }
                _ => {}
            }
        }
    }
    true
}

fn handle_args_popup(app: &mut App, key: event::KeyEvent) -> bool {
    if let Popup::ArgsPreview {
        ref mut content,
        ref mut editing,
        ref mut cursor_line,
    } = app.popup
    {
        if *editing {
            let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            if lines.is_empty() {
                lines.push(String::new());
            }
            match key.code {
                KeyCode::Up => *cursor_line = cursor_line.saturating_sub(1),
                KeyCode::Down => *cursor_line = (*cursor_line + 1).min(lines.len().saturating_sub(1)),
                KeyCode::Enter => {
                    // Insert new line after cursor
                    lines.insert(*cursor_line + 1, String::new());
                    *cursor_line += 1;
                    *content = lines.join("\n");
                }
                KeyCode::Backspace => {
                    let is_empty = lines.get(*cursor_line).map(|l| l.is_empty()).unwrap_or(false);
                    if is_empty && lines.len() > 1 {
                        lines.remove(*cursor_line);
                        *cursor_line = cursor_line.saturating_sub(1);
                    } else if let Some(line) = lines.get_mut(*cursor_line) {
                        line.pop();
                    }
                    *content = lines.join("\n");
                }
                KeyCode::Char(c) => {
                    if let Some(line) = lines.get_mut(*cursor_line) {
                        line.push(c);
                    }
                    *content = lines.join("\n");
                }
                KeyCode::Esc => {
                    // Save edited content as args override
                    app.args_file = None;
                    app.args_override = Some(content.clone());
                    *editing = false;
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Char('e') => {
                    *editing = true;
                    *cursor_line = 0;
                }
                KeyCode::Up => *cursor_line = cursor_line.saturating_sub(1),
                KeyCode::Down => {
                    let line_count = content.lines().count();
                    *cursor_line = (*cursor_line + 1).min(line_count.saturating_sub(1));
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.popup = Popup::None;
                }
                _ => {}
            }
        }
    }
    true
}

fn open_config_popup(app: &mut App) {
    let defaults = llamafile_core::args::default_config_values(&app.config.config_schema);
    let entries = crate::config_ui::flatten_schema(&app.config.config_schema, &app.config_values, &defaults);
    let first_interactive = entries.iter().position(|e| e.is_interactive()).unwrap_or(0);
    app.popup = Popup::ConfigEdit {
        entries,
        selected: first_interactive,
        editing: false,
        edit_buf: String::new(),
        dropdown_open: false,
        dropdown_selected: 0,
    };
}

fn rebuild_config_entries(app: &mut App) {
    let defaults = llamafile_core::args::default_config_values(&app.config.config_schema);
    let entries = crate::config_ui::flatten_schema(&app.config.config_schema, &app.config_values, &defaults);
    if let Popup::ConfigEdit { entries: ref mut e, .. } = app.popup {
        *e = entries;
    }
}

fn handle_confirm_popup(app: &mut App, key: event::KeyEvent) -> bool {
    if let Popup::Confirm { ref action, .. } = app.popup.clone() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                match action {
                    ConfirmAction::Quit => app.should_quit = true,
                }
                app.popup = Popup::None;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.popup = Popup::None;
            }
            _ => {}
        }
    }
    true
}

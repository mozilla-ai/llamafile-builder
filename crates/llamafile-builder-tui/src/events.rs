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
            KeyCode::Char('<') => {
                app.log_level = app.log_level.lower();
                app.log_scroll_offset = 0;
                return true;
            }
            KeyCode::Char('>') => {
                app.log_level = app.log_level.raise();
                app.log_scroll_offset = 0;
                return true;
            }
            KeyCode::Char('[') if app.log_expanded => {
                let total = app.filtered_logs().len() as u16;
                // Can't scroll more than total lines (renderer will clamp further by visible height)
                app.log_scroll_offset = (app.log_scroll_offset + 1).min(total);
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
        Popup::OutputEdit { .. } => handle_output_popup(app, key),
        Popup::ConfigEdit { .. } => handle_config_popup(app, key),
        Popup::GpuSelect { .. } => handle_gpu_popup(app, key),
        Popup::ArgsEdit { .. } => handle_args_edit(app, key),
        Popup::SaveRecipe { .. } => handle_save_recipe(app, key),
        Popup::LoadRecipe { .. } => handle_load_recipe(app, key),
        Popup::Confirm { .. } => handle_confirm_popup(app, key),
    }
}

fn handle_dashboard_key(app: &mut App, key: event::KeyEvent) -> bool {
    let has_mmproj = app.has_mmproj();
    let args_overridden = app.args_override.is_some();
    let local_model = app.is_local_model();
    match key.code {
        KeyCode::Down | KeyCode::Tab => {
            app.focus = app.focus.next(has_mmproj, args_overridden, local_model);
            true
        }
        KeyCode::Up | KeyCode::BackTab => {
            app.focus = app.focus.prev(has_mmproj, args_overridden, local_model);
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
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let default_name = app.recipe_id
                .and_then(|id| app.store.get(id).ok())
                .and_then(|r| r.name)
                .unwrap_or_default();
            app.popup = Popup::SaveRecipe { input: default_name };
            true
        }
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let recipes = app.list_recipes();
            app.popup = Popup::LoadRecipe { recipes, selected: 0 };
            true
        }
        KeyCode::Char(' ') if app.focus == Focus::Mmproj => {
            app.mmproj_disabled = !app.mmproj_disabled;
            true
        }
        KeyCode::Enter => {
            match app.focus {
                Focus::Version => {
                    // Find index of currently selected version in the flat list
                    let mut idx = 0;
                    let mut found = false;
                    for group in &app.config.versions {
                        for release in &group.releases {
                            if Some(&group.name) == app.version_group.as_ref()
                                && Some(&release.version) == app.version.as_ref()
                            {
                                found = true;
                                break;
                            }
                            idx += 1;
                        }
                        if found { break; }
                    }
                    if !found { idx = 0; }
                    app.popup = Popup::VersionSelect { selected: idx };
                }
                Focus::Model => {
                    // Scan local model sources
                    let ollama_models = llamafile_builder_core::ollama::get_ollama_models(None);
                    let lmstudio_models = llamafile_builder_core::lmstudio::get_lmstudio_models(None);

                    // Determine initial tab and cursor from current selection
                    let tab = app.model_source;
                    let selected = match tab {
                        ModelTab::Registry => app.model_id.as_ref()
                            .and_then(|id| app.config.models.iter().position(|m| &m.id == id))
                            .unwrap_or(0),
                        ModelTab::Ollama => app.ollama_model.as_ref()
                            .and_then(|sel| ollama_models.iter().position(|m| m.model_name == sel.model_name))
                            .unwrap_or(0),
                        ModelTab::LmStudio => app.lmstudio_model.as_ref()
                            .and_then(|sel| lmstudio_models.iter().position(|m| m.dir_name == sel.dir_name))
                            .unwrap_or(0),
                    };
                    app.popup = Popup::ModelSelect {
                        tab,
                        search: String::new(),
                        selected,
                        ollama_models,
                        lmstudio_models,
                    };
                }
                Focus::Weights => {
                    if let Some(model) = app.selected_model() {
                        if let Some(file) = model.files.iter().find(|f| f.role == "weights") {
                            let options: Vec<String> =
                                file.quantizations.iter().map(|q| q.name.clone()).collect();
                            if !options.is_empty() {
                                let idx = current_quant_index(app, "weights", &options);
                                app.popup = Popup::QuantSelect {
                                    role: "weights".to_string(),
                                    options,
                                    selected: idx,
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
                                    let idx = current_quant_index(app, "mmproj", &options);
                                    app.popup = Popup::QuantSelect {
                                        role: "mmproj".to_string(),
                                        options,
                                        selected: idx,
                                    };
                                }
                            }
                        }
                    }
                }
                Focus::Output => {
                    let current = app.effective_output();
                    let mut textarea = tui_textarea::TextArea::new(vec![current.clone()]);
                    textarea.set_block(
                        ratatui::widgets::Block::bordered()
                            .title(" Output Path ")
                            .title_bottom(" Esc=save & close ")
                    );
                    textarea.set_cursor_line_style(ratatui::style::Style::default());
                    textarea.move_cursor(tui_textarea::CursorMove::End);
                    app.popup = Popup::OutputEdit { textarea, original: current };
                }
                Focus::Config => {
                    open_config_popup(app);
                }
                Focus::Gpu => {
                    let available = app.available_gpu_libs();
                    let selected: Vec<bool> = available.iter()
                        .map(|opt| app.gpu_libs.contains(&(opt.accel, opt.os)))
                        .collect();
                    app.popup = Popup::GpuSelect {
                        available,
                        selected,
                        cursor: 0,
                    };
                }
                Focus::Build => {
                    if app.can_build() && !matches!(app.build_status, BuildStatus::Building { .. }) {
                        // Build will be triggered in the main loop
                        app.build_status = BuildStatus::Building {
                            phase: "starting".to_string(),
                            progress: 0.0,
                        };
                        app.build_started_at = Some(std::time::Instant::now());
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
                    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
                    let mut textarea = tui_textarea::TextArea::new(lines);
                    textarea.set_block(
                        ratatui::widgets::Block::bordered()
                            .title(" .args editor ")
                            .title_bottom(" Esc=save & close  Ctrl+R=reset to generated ")
                    );
                    textarea.set_cursor_line_style(ratatui::style::Style::default().fg(ratatui::style::Color::Yellow));
                    app.popup = Popup::ArgsEdit { textarea, original: content };
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
        ref mut tab,
        ref mut search,
        ref mut selected,
        ref ollama_models,
        ref lmstudio_models,
    } = app.popup
    {
        // Tab switching (left/right arrows)
        match key.code {
            KeyCode::Left => {
                *tab = tab.prev();
                *selected = 0;
                return true;
            }
            KeyCode::Right => {
                *tab = tab.next();
                *selected = 0;
                return true;
            }
            _ => {}
        }

        match *tab {
            ModelTab::Registry => {
                let filtered = llamafile_builder_core::search_models(
                    &app.config.models,
                    Some(search.as_str()).filter(|s| !s.is_empty()),
                    None,
                );
                let count = filtered.len();

                match key.code {
                    KeyCode::Up => *selected = selected.saturating_sub(1),
                    KeyCode::Down => *selected = (*selected + 1).min(count.saturating_sub(1)),
                    KeyCode::Enter => {
                        if let Some(model) = filtered.get(*selected) {
                            app.model_source = ModelTab::Registry;
                            app.model_id = Some(model.id.clone());
                            app.custom_model_file = None;
                            app.ollama_model = None;
                            app.lmstudio_model = None;
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
            ModelTab::Ollama => {
                let count = ollama_models.len();
                match key.code {
                    KeyCode::Up => *selected = selected.saturating_sub(1),
                    KeyCode::Down => if count > 0 {
                        *selected = (*selected + 1).min(count - 1);
                    },
                    KeyCode::Enter => {
                        if let Some(model) = ollama_models.get(*selected) {
                            app.model_source = ModelTab::Ollama;
                            app.ollama_model = Some(model.clone());
                            app.model_id = None;
                            app.custom_model_file = None;
                            app.lmstudio_model = None;
                            app.quantizations.clear();
                        }
                        app.popup = Popup::None;
                    }
                    KeyCode::Esc => app.popup = Popup::None,
                    _ => {}
                }
            }
            ModelTab::LmStudio => {
                let count = lmstudio_models.len();
                match key.code {
                    KeyCode::Up => *selected = selected.saturating_sub(1),
                    KeyCode::Down => if count > 0 {
                        *selected = (*selected + 1).min(count - 1);
                    },
                    KeyCode::Enter => {
                        if let Some(model) = lmstudio_models.get(*selected) {
                            app.model_source = ModelTab::LmStudio;
                            app.lmstudio_model = Some(model.clone());
                            app.model_id = None;
                            app.custom_model_file = None;
                            app.ollama_model = None;
                            app.quantizations.clear();
                        }
                        app.popup = Popup::None;
                    }
                    KeyCode::Esc => app.popup = Popup::None,
                    _ => {}
                }
            }
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

fn handle_output_popup(app: &mut App, key: event::KeyEvent) -> bool {
    // Esc saves and closes
    if key.code == KeyCode::Esc {
        if let Popup::OutputEdit { ref textarea, ref original } = app.popup {
            let content = textarea.lines().join("");
            if content != *original {
                app.output_path = Some(content);
            }
        }
        app.popup = Popup::None;
        return true;
    }

    // Block Enter (newlines) — output path must stay one line
    if key.code == KeyCode::Enter {
        return true;
    }

    // Forward everything else to the textarea
    if let Popup::OutputEdit { ref mut textarea, .. } = app.popup {
        textarea.input(key);
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
                    let mut idx = if *selected > 0 { *selected - 1 } else { 0 };
                    while !entries_clone.get(idx).map(|e| e.is_interactive()).unwrap_or(false) {
                        if idx == 0 { break; }
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
                        let defaults = llamafile_builder_core::args::default_config_values(&app.config.config_schema);
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
                        let defaults = llamafile_builder_core::args::default_config_values(&app.config.config_schema);
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
                        let defaults = llamafile_builder_core::args::default_config_values(&app.config.config_schema);
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
    if let Popup::GpuSelect {
        ref available,
        ref mut selected,
        ref mut cursor,
    } = app.popup
    {
        let len = available.len();
        match key.code {
            KeyCode::Up => *cursor = cursor.saturating_sub(1),
            KeyCode::Down => {
                if len > 0 {
                    *cursor = (*cursor + 1).min(len - 1);
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if *cursor < selected.len() {
                    selected[*cursor] = !selected[*cursor];
                }
            }
            KeyCode::Esc => {
                // Apply selections back to app state
                app.gpu_libs = available.iter().zip(selected.iter())
                    .filter(|(_, &checked)| checked)
                    .map(|(opt, _)| (opt.accel, opt.os))
                    .collect();
                app.popup = Popup::None;
            }
            _ => {}
        }
    }
    true
}

fn handle_args_edit(app: &mut App, key: event::KeyEvent) -> bool {
    if key.code == KeyCode::Esc {
        // Only save as override if the content was actually changed
        if let Popup::ArgsEdit { ref textarea, ref original } = app.popup {
            let content = textarea.lines().join("\n");
            if content != *original {
                app.args_file = None;
                app.args_override = Some(content);
            }
        }
        app.popup = Popup::None;
        return true;
    }

    // Ctrl+R = reset to generated (discard override)
    if key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.args_override = None;
        app.args_file = None;
        // Regenerate and reopen
        let content = if let Ok(builder) = app.make_builder() {
            builder.resolved_args()
        } else {
            "(error generating .args)".to_string()
        };
        let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        let mut textarea = tui_textarea::TextArea::new(lines);
        textarea.set_block(
            ratatui::widgets::Block::bordered()
                .title(" .args editor ")
                .title_bottom(" Esc=save & close  Ctrl+R=reset to generated ")
        );
        textarea.set_cursor_line_style(ratatui::style::Style::default().fg(ratatui::style::Color::Yellow));
        app.popup = Popup::ArgsEdit { textarea, original: content };
        return true;
    }

    // Forward all other keys to the textarea
    if let Popup::ArgsEdit { ref mut textarea, .. } = app.popup {
        textarea.input(crossterm::event::Event::Key(key.into()));
    }
    true
}

fn open_config_popup(app: &mut App) {
    let defaults = llamafile_builder_core::args::default_config_values(&app.config.config_schema);
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
    let defaults = llamafile_builder_core::args::default_config_values(&app.config.config_schema);
    let entries = crate::config_ui::flatten_schema(&app.config.config_schema, &app.config_values, &defaults);
    if let Popup::ConfigEdit { entries: ref mut e, .. } = app.popup {
        *e = entries;
    }
}

/// Find the index of the currently selected quantization in an options list.
/// Falls back to 0 (first/default) if no explicit selection exists.
fn current_quant_index(app: &App, role: &str, options: &[String]) -> usize {
    app.quantizations.get(role)
        .and_then(|name| options.iter().position(|o| o.eq_ignore_ascii_case(name)))
        .unwrap_or(0)
}

fn handle_save_recipe(app: &mut App, key: event::KeyEvent) -> bool {
    if let Popup::SaveRecipe { ref mut input } = app.popup {
        match key.code {
            KeyCode::Enter => {
                if !input.is_empty() {
                    let name = input.clone();
                    match app.save_recipe(&name) {
                        Ok(()) => {
                            app.add_log(LogLevel::Info, format!("Recipe saved as '{}'", name));
                        }
                        Err(e) => {
                            app.add_log(LogLevel::Error, format!("Failed to save recipe: {}", e));
                        }
                    }
                }
                app.popup = Popup::None;
            }
            KeyCode::Esc => app.popup = Popup::None,
            KeyCode::Backspace => { input.pop(); }
            KeyCode::Char(c) => input.push(c),
            _ => {}
        }
    }
    true
}

fn handle_load_recipe(app: &mut App, key: event::KeyEvent) -> bool {
    if let Popup::LoadRecipe { ref recipes, ref mut selected } = app.popup {
        let count = recipes.len();
        match key.code {
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Down => if count > 0 { *selected = (*selected + 1).min(count - 1); },
            KeyCode::Enter => {
                if let Some(recipe) = recipes.get(*selected) {
                    let recipe = recipe.clone();
                    let name = recipe.name.clone().unwrap_or_else(|| recipe.id.to_string());
                    app.load_from_recipe(&recipe);
                    app.add_log(LogLevel::Info, format!("Loaded recipe '{}'", name));
                }
                app.popup = Popup::None;
            }
            KeyCode::Esc => app.popup = Popup::None,
            _ => {}
        }
    }
    true
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

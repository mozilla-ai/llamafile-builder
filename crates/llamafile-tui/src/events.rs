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

    // Log level: + = more verbose, - = less verbose
    if matches!(key.code, KeyCode::Char('+')) && matches!(app.popup, Popup::None) {
        app.log_level = app.log_level.lower();
        return true;
    }
    if matches!(key.code, KeyCode::Char('-')) && matches!(app.popup, Popup::None) {
        app.log_level = app.log_level.raise();
        return true;
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
        Popup::ArgsPreview { .. } => {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter) {
                app.popup = Popup::None;
            }
            true
        }
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
                            if options.len() > 1 {
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
                    if let Some(model) = app.selected_model() {
                        if let Some(file) = model.files.iter().find(|f| f.role == "mmproj") {
                            let options: Vec<String> =
                                file.quantizations.iter().map(|q| q.name.clone()).collect();
                            if options.len() > 1 {
                                app.popup = Popup::QuantSelect {
                                    role: "mmproj".to_string(),
                                    options,
                                    selected: 0,
                                };
                            }
                        }
                    }
                }
                Focus::Output => {
                    let current = app.effective_output();
                    app.popup = Popup::OutputEdit { input: current };
                }
                Focus::Config => {
                    let defaults =
                        llamafile_core::args::default_config_values(&app.config.config_schema);
                    let mut entries: Vec<(String, String)> = defaults
                        .iter()
                        .map(|(k, v)| {
                            let val = app
                                .config_values
                                .get(k)
                                .unwrap_or(v);
                            (k.clone(), val.to_string())
                        })
                        .collect();
                    entries.sort_by(|a, b| a.0.cmp(&b.0));
                    app.popup = Popup::ConfigEdit {
                        entries,
                        selected: 0,
                        editing: false,
                        edit_buf: String::new(),
                    };
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
                        app.log_expanded = true;
                    }
                }
                Focus::ShowArgs => {
                    if let Ok(builder) = app.make_builder() {
                        let content = builder.resolved_args();
                        app.popup = Popup::ArgsPreview { content };
                    }
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
    if let Popup::ConfigEdit {
        ref mut entries,
        ref mut selected,
        ref mut editing,
        ref mut edit_buf,
    } = app.popup
    {
        if *editing {
            match key.code {
                KeyCode::Enter => {
                    if let Some((key_name, _)) = entries.get(*selected) {
                        let parsed: serde_json::Value = serde_json::from_str(edit_buf)
                            .unwrap_or_else(|_| serde_json::Value::String(edit_buf.clone()));
                        app.config_values.insert(key_name.clone(), parsed.clone());
                        entries[*selected].1 = parsed.to_string();
                    }
                    *editing = false;
                    edit_buf.clear();
                }
                KeyCode::Esc => {
                    *editing = false;
                    edit_buf.clear();
                }
                KeyCode::Backspace => {
                    edit_buf.pop();
                }
                KeyCode::Char(c) => edit_buf.push(c),
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Up => *selected = selected.saturating_sub(1),
                KeyCode::Down => {
                    *selected = (*selected + 1).min(entries.len().saturating_sub(1))
                }
                KeyCode::Enter => {
                    if let Some((_, val)) = entries.get(*selected) {
                        *edit_buf = val.clone();
                        *editing = true;
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

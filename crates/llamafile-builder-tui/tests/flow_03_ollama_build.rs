//! Flow 3 — Ollama build. Switch the Model Select popup to the Ollama tab,
//! pick one of the fake-scaffolded Ollama models, and verify
//! `prepare_build()` carries the Ollama selection (and clears any registry
//! `model_id`).

mod support;

use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::local_sources::make_ollama_dir;
use support::render;
use support::snapshots;

#[test]
fn ollama_build_pick_llama3_8b() {
    let _settings = snapshots::settings().bind_to_scope();
    let TestApp { mut app, .. } = TestApp::new();

    // Scaffold a fake Ollama install with two model:tag entries. The dir
    // is owned by the test (TempDir auto-cleans on drop); we keep the
    // handle alive in scope so the dir survives until the assertions run.
    let ollama_root = tempfile::TempDir::new().expect("ollama tempdir");
    make_ollama_dir(
        ollama_root.path(),
        &[
            ("llama3", "8b", "deadbeef01"),
            ("llama3", "70b", "deadbeef02"),
            ("phi3", "mini", "deadbeef03"),
        ],
    );
    app.ollama_dir = Some(ollama_root.path().to_string_lossy().to_string());

    let mut term = render::terminal();

    // Move from Version → Model and open the Model Select popup. Default
    // tab is Registry.
    dispatch::keys(&mut app, &[down(), enter()]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("01_model_popup_registry_tab", frame);

    // Switch to the Ollama tab (right arrow). The fake install's three
    // entries should appear sorted (llama3:70b, llama3:8b, phi3:mini per
    // the core scanner's natural ordering).
    dispatch::keys(&mut app, &[right()]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("02_model_popup_ollama_tab", frame);

    // Pick the second entry (down → Enter). The exact ordering is the
    // scanner's; we don't assert which model lands — only that the
    // selection round-trips through SavedState.
    dispatch::keys(&mut app, &[down(), enter()]);

    // Local-model selection skips Weights/Mmproj rows: focus jumps to
    // Output. The dashboard now reads "(local model)" or similar.
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("03_dashboard_after_ollama_select", frame);

    // SavedState assertion: Ollama model populated, registry model_id
    // cleared, no quantizations set (Ollama models are pre-quantized).
    insta::assert_debug_snapshot!("04_prepare_build", app.prepare_build());
}

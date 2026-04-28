//! Flow 1 — registry build. The flagship happy-path: pick a version, search
//! and pick a registry model with multiple quants, change the weights
//! quantization, and verify `prepare_build()` reflects every selection.

mod support;

use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::render;

#[test]
fn registry_build_pick_mistral_bf16() {
    let TestApp { mut app, .. } = TestApp::new();
    let mut term = render::terminal();

    // Decision point 1: dashboard at default (Version focused, Qwen3 0.6B
    // selected by default per fixtures). Snapshot the starting state so
    // changes downstream are easy to spot in diffs.
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("01_dashboard_default", frame);

    // Open the Version popup. Default version (0.10.1_alpha) is already
    // what we want — re-select it to exercise the popup path.
    dispatch::keys(&mut app, &[enter()]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("02_version_popup_open", frame);
    dispatch::keys(&mut app, &[enter()]);

    // Move focus from Version → Model and open the Model Select popup.
    dispatch::keys(&mut app, &[down(), enter()]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("03_model_popup_open", frame);

    // Filter to Mistral 3B (which has two quants — Q4_K_M and BF16) and
    // select it. Filtering proves the search box plumbs `selected = 0`
    // when it changes.
    dispatch::keys(&mut app, &type_str("ministral"));
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("04_model_popup_filtered", frame);
    dispatch::keys(&mut app, &[enter()]);

    // Move focus to Weights (skipping the now-unfocused intermediate rows
    // is handled by `Focus::next`), open the Quantization popup, pick the
    // *second* option (BF16) — proves the down arrow + Enter actually
    // changes selection, not just confirms the default.
    dispatch::keys(&mut app, &[down(), enter()]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("05_quant_popup_open", frame);
    dispatch::keys(&mut app, &[down(), enter()]);

    // Final dashboard: Mistral 3B selected, weights at BF16.
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("06_dashboard_after_selections", frame);

    // Failure-mode-D assertion: the SavedState that would drive the build
    // matches the user's selections.
    insta::assert_debug_snapshot!("07_prepare_build", app.prepare_build());
}

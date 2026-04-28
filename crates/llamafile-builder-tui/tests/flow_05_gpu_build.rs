//! Flow 5 — GPU-accelerated build. Open the GPU popup, multi-select two
//! libraries with Space, escape to apply, and assert `prepare_build()`
//! carries the expected GPU specs.

mod support;

use llamafile_builder_tui::app::Focus;
use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::render;

#[test]
fn gpu_build_select_cuda_and_rocm_linux() {
    let TestApp { mut app, .. } = TestApp::new();
    let mut term = render::terminal();

    // Default state: Qwen3 0.6B, no mmproj, no GPU. Move focus to GPU
    // and open the popup.
    dispatch::focus_on(&mut app, Focus::Gpu);
    dispatch::keys(&mut app, &[enter()]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("01_gpu_popup_open", frame);

    // Cursor starts at index 0 (Cuda Linux). Space toggles. Then down ×2
    // to reach Rocm Linux (entries are ordered cuda-linux, cuda-windows,
    // rocm-linux, …) and toggle that too.
    dispatch::keys(&mut app, &[space(), down(), down(), space()]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("02_gpu_popup_two_selected", frame);

    // Esc applies and closes the popup.
    dispatch::keys(&mut app, &[esc()]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("03_dashboard_with_gpu", frame);

    // Failure-mode-D: the SavedState now lists both GPU libs in the
    // builder's parse format (`accel:os`).
    let saved = app.to_saved_state();
    assert_eq!(saved.gpu_libs, vec!["cuda:linux", "rocm:linux"]);
    insta::assert_debug_snapshot!("04_prepare_build", saved);
}

# TUI Testing Specification

Status: draft. Audience: Davide + Claude Code. This spec is the working brief for adding automated TUI tests; both should be able to implement against it.

## 1. Goals and non-goals

**Primary goal.** Enable automated verification that the TUI behaves as advertised, so iterating on the TUI doesn't require manually walking many build paths. Concretely: when Claude changes a TUI behavior, a test suite should catch regressions across all the documented flows in [`specs/TUI.md`](TUI.md) without a human running the binary.

**Secondary goals.** Regression coverage for keys, navigation, state mapping, and log-driven progress. Documentation-via-snapshots for what each screen looks like in each meaningful state.

**Non-goals.** Cross-terminal-emulator visual fidelity. Performance. Accessibility. Fuzz/property testing of input. These are deferred to §10.

## 2. The bug class we're targeting

The umbrella concern is *"the tool doesn't do what it claims"*. That decomposes into five failure modes; the test plan is organized around catching each:

| Code | Failure | Caught by |
|---|---|---|
| A | User pressed a key; app's understanding of intent is wrong | Level 1 — state transition tests |
| B | Intent understood, but app state didn't update correctly | Level 1 + Level 3 |
| C | State correct, but display doesn't reflect it (visual lie) | Level 2 — rendering snapshots |
| D | State correct, but the build invocation doesn't match the state | Level 3 — flow tests with `SavedState` assertion |
| E | Build runs, but TUI shows wrong phase or progress | Level 4 — log-driven progress tests |

If a test doesn't clearly catch one of A–E, it's probably not worth writing.

## 3. Current state of testing

Snapshot of the workspace as of this revision:

- **`llamafile-builder-core`**: ~136 unit tests across `state`, `args`, `download`, `builder`, `config_store`, `cache`, `config_manager`, `disk_space`, `validation`, `ollama`, `lmstudio`. Solid coverage of model selection, schema-driven `.args` generation, recipes, and validation.
- **`llamafile-builder` (CLI)**: 15 tests in `server.rs` (REST endpoints against a real `Router`) plus 1 in `state_file.rs`. JSON-RPC dispatch in `interactive.rs` is currently uncovered.
- **`llamafile-builder-tui`**: 36 unit tests in `src/` (23 in `app.rs`, 9 in `events.rs`, 4 in `main.rs`) plus 24 integration tests under `tests/` — 11 flow tests (`flow_01`…`flow_11`), `log_layer_integration.rs` (7), `smoke.rs` (2).
- **`insta` is wired** as a workspace dev-dependency; ~38 `.snap` files committed under `crates/llamafile-builder-tui/tests/snapshots/`.
- **CI**: `.github/workflows/release.yml` exists for cross-compiled releases. There is no test-on-PR workflow yet (Phase 5 below).

Coverage now spans failure modes A, B, C, and D for the documented flows. Failure mode E (log-driven progress) is partially covered via `log_layer_integration.rs` but not exhaustively per `BuildPhase` variant.

## 4. Architectural prerequisites

Two small refactors unlock everything below. Neither is a rewrite; both are independently good.

### 4.1 Extract `App::prepare_build() -> SavedState`

Today, the build is triggered inside the main loop's `BuildStatus::Building { phase: "starting" }` branch by reconstructing a `Builder` from app state and calling `tokio::spawn`. The reconstruction logic is inline.

**Refactor.** Extract that reconstruction into `App::prepare_build(&self) -> SavedState` (or `BuildState` — whichever is the natural input to `Builder`). The main loop calls it, then spawns the builder with the result. Tests can call it directly after driving the app to a "ready to build" state, and assert on the returned state without ever spawning a build.

This is the assertion point for failure mode D.

### 4.2 Replace regex log-content matching with structured tracing fields

Today, `TuiLogLayer` extracts phase and download progress by regex-matching log message text (e.g., `"download started"`, `"bytes_downloaded="`). This couples the TUI to log copy. Editing a `tracing::info!("downloading model")` to `"fetching model"` silently breaks the progress bar — exactly the "tool doesn't do what it claims" hazard.

**Refactor.** Define a closed enum of phases in core. The implementation
ships these variants, in pipeline order: `BuildPhase::PreparingZipalign`,
`PreparingRuntime`, `PreparingModel`, `WritingArgs`, `PreparingGpuLibs`,
`Assembling`. Emit them as structured tracing fields, using the `%` (Display)
formatter so the value lands as the stable kebab-case identifier:

```rust
tracing::info!(phase = %BuildPhase::PreparingModel, filename = %name, "downloading model");
```

`TuiLogLayer` reads the `phase` field directly from the `tracing::Event`'s
field visitor, never the message text. **Crucial implementation detail**:
`%`-formatted values (`DisplayValue<T>`) land in `Visit::record_debug`, not
`record_str` — the visitor's debug arm is the one that must extract the
`phase` and `filename` fields. The kebab-case Display form is the *only*
canonical wire format: `BuildPhase::from_str` strictly parses it and
nothing else. An emit site that mistakenly uses `?` (Debug) produces a
PascalCase value that `from_str` rejects, so the `phase` signal goes
missing on the consumer side and the `log_layer_integration` tests fail
on the next run — drift surfaced early instead of silently widened. Same
for `bytes_downloaded` / `bytes_total` — already candidates for structured
fields, just not enforced.

This is the contract surface for failure mode E.

### 4.3 What stays unchanged

No `Executor` trait. `SavedState` is the assertion point and the build itself is already exercised by 121 core tests. Adding an executor abstraction would be speculative.

## 5. Test levels

### Level 1 — State transition tests (extend existing)

**What.** Pure unit tests on the `update`/event-dispatch logic. Each key binding, focus rule, popup edit-mode transition, and boundary condition.

**Tooling.** Plain `#[test]`, no `TestBackend`. Build app state, dispatch a `KeyEvent`, assert resulting state.

**Coverage gap to close.** The existing 29 tests cover top-level navigation and a few popups. Missing: every popup's edit-mode transitions (Quantization radio-list, Configuration schema-driven editor, GPU multi-select, Output text-edit, Save/Load Recipe), the args-override locking the Config row, multi-tab Model Select switching.

**Done when.** Every key binding documented in [`specs/TUI.md`](TUI.md) has at least one transition test.

### Level 2 — Component rendering snapshots (lower priority)

**What.** Each popup and the dashboard rendered to a `Buffer` via `TestBackend`, snapshotted in its meaningful states (empty / loading / error / populated / focused / disabled).

**Tooling.** `ratatui::backend::TestBackend` + `insta`. Render to buffer, format buffer to string (preserving styles where semantically meaningful), `insta::assert_snapshot!`.

**Why lower priority.** The Q13 pain points (keys, state-not-reflected, wrong command, broken navigation) aren't pure visual bugs. Level 3 flow tests already snapshot screens at decision points along the paths users actually take, which covers most of failure mode C. Level 2 picks up off-path states (e.g., the Model Select popup in its loading state) that aren't reached by any flow test.

**Add as gaps surface.** When a visual-only regression slips through, write the missing component snapshot at that point. Don't enumerate the full matrix upfront.

### Level 3 — Flow tests (highest priority)

**What.** Whole-app tests driving scripted event sequences through `TestBackend`, snapshotting the rendered buffer at decision points and asserting on `App::prepare_build()` output at the build trigger.

**Tooling.** `TestBackend` + `insta` for screen snapshots; direct `assert_eq!` / `insta::assert_debug_snapshot!` on `SavedState` at build trigger.

**Pattern.**

```rust
#[test]
fn registry_build_flow_qwen_q4() {
    let mut app = App::new_for_test(/* fixtures */);
    let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();

    drive(&mut app, &mut term, &[enter(), down(), enter()]); // open Version, pick first
    insta::assert_snapshot!(render(&term));

    drive(&mut app, &mut term, &[/* navigate to Model, pick Qwen3-0.6B */]);
    drive(&mut app, &mut term, &[/* navigate to Build button, press Enter */]);

    let saved = app.prepare_build();
    insta::assert_debug_snapshot!(saved);
}
```

**Coverage targets** (one flow test each, minimum):

| # | Flow | Entry | Steps | Build-trigger assertion |
|---|---|---|---|---|
| 1 | Registry build | dashboard | version → registry model → quant → output → build | `SavedState` has correct version, registry model id, quant, output |
| 2 | Custom model build | dashboard | version → custom weights file → output → build | `SavedState.custom_model_files` populated, `selected_model_id` empty |
| 3 | Ollama build | dashboard | switch tab → select Ollama model → output → build | `SavedState.selected_ollama_model` set |
| 4 | LM Studio build | dashboard | switch tab → select LMS model → output → build | `SavedState.selected_lmstudio_model` set |
| 5 | GPU-accelerated build | dashboard | flow 1 + GPU multi-select | `SavedState` has expected gpu libs |
| 6 | Recipe save | mid-config | Ctrl+S → name → confirm | recipe file written; `SavedState` matches in-memory |
| 7 | Recipe load | dashboard | Ctrl+L → list → select | app state matches loaded recipe |
| 8 | Config override (.args edit) | mid-config | Ctrl+R → edit text → close | Config row locked; `SavedState` carries override |
| 9 | Build cancel mid-flight | building | Ctrl+C → confirm yes | cancel flag set; build status transitions to cancelled |
| 10 | Build error display | failed | (synthetic failure) | error popup shown with correct text; dismissible |

**Done when.** All 10 flows have a passing test that asserts both rendered screens at decision points and the final `SavedState` (where applicable). See §12 for how to add the 11th.

### Level 4 — Log-driven progress tests

**What.** Feed synthetic structured tracing events into `TuiLogLayer`; assert `BuildStatus` and `DownloadInfo` transition correctly.

**Tooling.** Construct a `tracing::Event` (or use `tracing::dispatcher::with_default` to scope a subscriber) and inject events directly. Drive the app's tick loop and inspect `app.build_status` / `app.download_info`.

**Coverage targets.**

- Each `BuildPhase` variant produces the expected `BuildStatus { phase: ... }`.
- Download events with `bytes_downloaded` / `bytes_total` produce correct `DownloadInfo` and percentages.
- Out-of-order or missing events don't corrupt status (e.g., a `ModelReady` without a preceding `ModelDownloading` should still leave UI consistent).
- Log filter levels (DEBUG/INFO/WARN/ERROR) gate display correctly.

**Done when.** Every `BuildPhase` variant has a test, plus the boundary cases above.

### Level 5 — pty end-to-end (deferred)

**What.** Real terminal subprocess for things only observable there: SIGWINCH/resize handling, real async I/O timing, signals.

**Tooling.** A pty crate (e.g., `portable-pty` or `expectrl`).

**Add only when** a real bug surfaces that L1–L4 miss. Don't write speculative pty tests; they're slow and flaky.

## 6. Tooling and conventions

- **`insta`** added as workspace dev-dependency. `cargo insta review` for accepting diffs. Snap files committed.
- **`TestBackend`** at canonical 80×24. Multi-column popups (Model Select with tabs + filter + list) tested at 120×40 as well.
- **Key event helpers.** A `crates/llamafile-builder-tui/tests/support/keys.rs` module with `enter()`, `tab()`, `down()`, `char('q')`, `ctrl('s')`, `esc()`, etc. Keep tests readable.
- **Time control.** `tokio::time::pause()` + `advance()` for any flaky-on-timing path. Don't `sleep` in tests.
- **Buffer rendering.** A helper `render(&Terminal) -> String` that includes style markers (e.g., `[bold]…[/bold]`, `[red]…[/red]`) only where styles are semantically meaningful (errors, focus). Pure-text output for everything else, to keep snapshots reviewable.
- **Test fixtures.** A `support::fixtures` module providing canned `versions.json`/`models.json`/`config.json` so tests don't depend on remote state or the bundled defaults shifting.
- **Naming.** `flow_<n>_<short_name>.rs` for flow tests, one per file or grouped by feature; `popup_<name>_render.rs` for component snapshots; `log_<phase>.rs` for L4.

## 7. Phased implementation

| Phase | Status | Work | Outcome |
|---|---|---|---|
| 0 | done | Refactors §4.1 + §4.2 | `prepare_build()` exists; phase emitted as structured field |
| 1 | done | Add `insta`, set up `TestBackend` harness, write `support` helpers (keys, fixtures, render) | Infra ready; one trivial rendering snapshot proves the pipeline |
| 2 | done | L3 flow tests for the 10 flows in §5 (plus `flow_11_custom_files_round_trip`) | Failure modes B + D covered for documented flows |
| 3 | partial | L4 log-driven progress tests | `log_layer_integration.rs` covers the visitor + a subset of phases; not yet one test per `BuildPhase` variant |
| 4 | not started | L1 gap-fill (popup edit-mode transitions, args override locking, etc.) | Failure mode A fully covered |
| 5 | not started | Test-on-PR workflow in `.github/workflows/`. Run `cargo test --workspace`, fail on snapshot drift unless reviewed | Regressions caught automatically |
| later | not started | L2 component snapshots, added as visual regressions surface | Failure mode C covered opportunistically |

Phases 0–2 are the critical path for the stated pain and are complete; Phases 3–5 are the remaining work before this spec can be considered fully delivered.

## 8. Done criteria

| Criterion | Status |
|---|---|
| Every flow in §5 table has a passing flow test asserting both rendered state and `SavedState` | done (10/10, plus `flow_11`) |
| `insta` is wired and snapshot files are committed | done |
| `cargo test --workspace` runs the whole TUI suite in <30s on a developer laptop | done |
| Every key binding in [`specs/TUI.md`](TUI.md) has a state transition test | partial (Phase 4) |
| Every `BuildPhase` variant has a log-driven progress test | partial (Phase 3) |
| Test-on-PR CI runs the whole workspace and gates snapshot drift | not started (Phase 5) |

L2 (component rendering) is intentionally not in the done criteria; it's add-as-needed.

## 9. Test boundaries and observation points

- **TUI is tested against the core library via direct calls** (not JSON-RPC), matching production architecture.
- **`SavedState` returned from `prepare_build()` is the primary assertion surface for failure mode D.** It's the contract between the TUI and the builder; if it's correct, everything downstream is already covered by 121 core tests.
- **`TuiLogLayer` is the boundary for failure mode E.** Tests inject synthetic events at this boundary; they don't depend on a real build emitting them.
- **JSON-RPC is tested separately** as an integration test of the RPC layer itself, because external clients depend on it. Out of scope for this spec.

## 10. Out of scope / known gaps

Explicitly deferred:

- pty/e2e tests beyond §5 Level 5.
- Cross-terminal-emulator visual diff.
- Fuzz / property-based input testing.
- Accessibility audit.
- Performance benchmarks of the render loop.
- Full L2 component snapshot matrix (added opportunistically, see §5 Level 2).

## 11. Test maintenance policy

- **Snapshot diffs reviewed in PR.** A snapshot diff without a matching code change in the same PR is a red flag — either an unintentional regression or evidence the snapshot is testing something noisier than intended.
- **Bulk-accept (`cargo insta accept`) only when the PR description names the UI change driving the diff.** Avoids snapshot tests becoming rubber stamps.
- **Snap file rot.** If a `.snap` is modified more than three times in unrelated PRs, treat it as evidence the underlying assertion is too coarse — replace with a finer-grained state assertion or split.
- **Failure mode discipline.** When a TUI bug ships, classify it as A–E from §2 and write the missing test at the corresponding level before fixing. Same rule as the existing `is_user_cancellation` regression-guard pattern in `main.rs`.

## 12. How to add a flow test

The §5 table started at 10 flows; `flow_11_custom_files_round_trip` already lives in `crates/llamafile-builder-tui/tests/`. Adding the 12th should take ~10 minutes now that Phase 1's harness exists. Recipe:

1. **Identify the flow.** A flow is a sequence of user actions ending in a build trigger or a state-affecting action (save recipe, load recipe, etc.). If it's a single popup interaction with no surrounding context, write a Level 1 test instead.
2. **Pick fixtures.** Reuse the `support::fixtures` canned `versions.json`/`models.json`/`config.json` if possible. Add a new fixture only if no existing one covers the data needs (e.g., a new GPU library variant).
3. **Create the test file.** `crates/llamafile-builder-tui/tests/flows/flow_<n>_<short_name>.rs`. Follow the pattern in §5 Level 3.
4. **Drive the app via key helpers, not internal mutators.** Call `app.handle_event(KeyEvent...)`; don't reach into `app.selected_model_id = ...`. The whole point is to test that key dispatch produces the right state.
5. **Snapshot at decision points, not every keystroke.** A decision point is anywhere a user faces a fork: popup just opened, list filtered, dropdown changed, build button focused. Use `insta::assert_snapshot!(render(&term))`. Snapshotting on every key produces noisy `.snap` files and rubber-stamp reviews.
6. **Assert at the build trigger.** Call `app.prepare_build()` and `insta::assert_debug_snapshot!(saved)`. Don't actually spawn a build — that's already covered by 121 core tests.
7. **Run `cargo insta review`.** Read each new snapshot top-to-bottom before accepting. If anything looks unfamiliar, the test is wrong, not the snapshot.
8. **Update the §5 table.** Add a row so future readers know the flow exists. Keep the spec current.

**Common pitfalls.**

- Bypassing `handle_event` to set state directly. This skips the test of intent capture (failure mode A) — the most common TUI bug class.
- **Pre-stamping the value the production path is meant to derive.** A test that does `app.build_status = Building { phase: "downloading", … }` "to set up state" passes happily even if the layer that *should* set that field (here, `TuiLogLayer` extracting the `phase` tracing field) is broken or never runs. The shipped-and-reverted dead-code bug from commit 2f9ef8d is the canonical case: every flow test went green while the progress bar was permanently stuck. Rule: if production code derives a value, the test must drive whatever derives it. Synthetic injection is acceptable only at a *named boundary* the spec calls out (e.g. "`SavedState` is the L3 assertion point") — and even then, prefer driving the layer above. When you must inject, write a sibling unit test that drives the bypassed layer, so a regression there isn't invisible.
- Snapshotting too aggressively. One `assert_snapshot!` every 3–5 keys is usually right; one every keystroke is too much.
- Asserting on text that shifts with config-driven labels (e.g., model display names from `models.json`). Use stable fixtures from `support::fixtures` instead of bundled defaults.
- Forgetting to drive a tick after dispatching a key. Some state updates (download progress, async completion) only land on the next tick.
- Using `tokio::time::sleep` instead of `tokio::time::pause` + `advance`. The first introduces flake; the second is deterministic.

If adding a flow surfaces a missing helper (e.g., a new key combo, a new render assertion shape), put it in `support/` so the next flow benefits.

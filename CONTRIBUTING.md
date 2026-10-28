# Contributing to LabCRAFT

## Ground rules

1. **All edits to a VI go through `Command`** (`crates/labcraft-core/src/model.rs`). Never mutate `Vi` directly from UI code — that is what keeps undo, validation, and the future agent API consistent. New capability usually means a new `Command` variant plus an `exec` arm.
2. **Core stays GUI-free.** `labcraft-core` must not depend on egui/eframe. Anything an agent might need (validation, typing, running) belongs there.
3. **Test in the core first.** Build VIs with `Editor` + `Command` in a test and call `run()`; see `crates/labcraft-core/src/eval.rs` (`mod tests`) and `structure_tests.rs` for the pattern. UI code gets a render smoke test (see `app.rs` tests) that runs egui headlessly.
4. **Zero warnings, all tests green:** `cargo build` and `cargo test` must be clean before a PR.
5. **Match the surrounding style.** Comment sparingly, only for non-obvious behaviour.

## Workflow

```sh
cargo test                       # everything
cargo test -p labcraft-core      # fast loop for language/runtime work
cargo run                        # try it in the editor
```

- Branch from `main`, keep PRs focused (one feature or fix), reference the milestone issue.
- If you change semantics (typing rules, evaluation order), add a test that pins the behaviour.
- Update `docs/ROADMAP.md` when you finish or defer an item.

## Reporting UI problems

The UI was developed without screenshot tooling in CI, so visual issues are the most likely bugs. Please include a screenshot, the example/VI you were in, and what you right-clicked or dragged.

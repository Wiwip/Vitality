# SPRINT-SUMMARY — sprint/2026-11-07 (cycle 4, base `auto/2026-11-07`)

Stopped because the queue emptied after one item. Scout ran with "an empty list is a correct answer" and came back with a single test-only item, the hole that cycle 3's verdict measured and delegated rather than queuing. It dismissed two further leads with reasons the controller accepted: `sub_tasks`'s `.expect` still fails the test and dropping `Tasks` from `#[require]` would cascade anyway, and a duplicated `TaskMachine` start already shows up as an emptied `Tasks`, so a dedicated observer adds specificity without coverage.

| item | commit | verdict | one line |
| --- | --- | --- | --- |
| T-7 close the `ScenePlugin` guard hole against the duplicate-add mutation | `a08f8b7` | OK | a host that adds `ScenePlugin`/`DefaultPlugins` before vitality is now covered, by the panic that bug actually causes |

Gate at the tip: `bash scripts/gate.sh` exits 0, 11 tests (5 lib + 5 ability + 1 systems). The controller's pre-landing run, the summary landing run and the judge's re-run all printed `Compiling vitality v0.2.0`; `land.sh`'s own `item T-7` log replayed cached binaries and printed none. `cargo fmt --all -- --check` and `cargo clippy --offline --all-targets` both exit 0 — clippy verified at this tip by the cycle-4 judge's reviewer, not by the controller.

## Reviewer notes the owner should read

- **The new lines look like a vacuous assertion, and the reviewer checked the alternatives and kept it.** `assert!(app2.is_plugin_added::<ScenePlugin>())` can never be false: line 401 adds the plugin to `app2` itself, and bevy `0.20.1` has no `remove_plugin`, no `try_add_plugins`, and keeps `plugin_names`/`plugin_registry` `pub(crate)` (`sub_app.rs:70,73`), while the only `Result`-returning installer, `App::add_boxed_plugin`, is also `pub(crate)` (`app.rs:524`). So the pin is the panic raised inside `app2.add_plugins(AttributesPlugin)` when `AttributesPlugin` tries to install a second unique plugin, and that panic is not an artifact of the test — it is what happens to a real host at startup. The reviewer built the `catch_unwind` alternative and rejected it: same coverage, needs `AssertUnwindSafe`, and loses the useful `panicked at src/lib.rs` frame. Not measured, and the better form if you want it: `#[should_panic(expected = "plugin was already added in application")]` on a test holding only the `app2` block, which is bevy's own idiom (`bevy_app-0.20.1/src/app.rs:1697-1701`) and would keep the two claims separately attributable.
- **Which wrong edit trips which assert, now measured rather than argued:** removing the guard while keeping the install unconditional → the panic; skipping or inverting the install → T-5's first assert (`AttributesPlugin must install ScenePlugin even when Assets<ScenePatch> already exists`, which fires first); the new trailing assert on its own → nothing, under any of them.
- **One test function now carries two claims and its name describes only one.** `scene_plugin_is_installed_when_the_host_only_inited_the_asset` also covers "the host already added `ScenePlugin`". The name was left alone on purpose: it is quoted verbatim in `SPRINT-TODO.md`, `SPRINT-LOG.md` and verdict 3, all of which are protected from worker edits mid-loop. If you want it renamed, do it in a commit of your own and the three references can move with it.

## Edge cases

- T-7: a panic in the second half of that test hides the first half's state from the report, and `cargo test scene_plugin` runs both.
- T-7: if the `ScenePlugin` install is ever gated behind a feature or cfg, only T-5's first assert catches it; the trailing assert stays green.
- Carry-over from cycle 3, unpinned and owned by you: `ScenePlugin::build` calls `init_asset::<ScenePatch>()`, which overwrites, so a host that loads `ScenePatch` assets before adding vitality loses them. No test covers it and none of this sprint touched it.
- All cycles: this branch does not build on its own — the bevy `0.20` bump and `hfsm_bevy = { path = "../hfsm-rs/crates/hfsm_bevy" }` live in your uncommitted `Cargo.toml`, which `land.sh` never stages.

## Try it

1. Break the guard the way the suite used to miss. In a scratch copy replace `src/lib.rs:102-105` with `app.add_plugins(ScenePlugin);` and run `cargo test --test test_ability_machine`: `scene_plugin_is_installed_when_the_host_only_inited_the_asset` dies with `panicked at src/lib.rs:103:13 … plugin was already added in application`. Before this sprint that mutation passed all 11 tests.
2. See that the assertion is not what catches it: comment out the trailing `assert!` and repeat step 1 — still red. That is the shape bevy forces on you here.
3. Look at the untested host yourself: `App::new().add_plugins((MinimalPlugins, AssetPlugin::default(), ScenePlugin)).add_plugins(AttributesPlugin)` runs clean on the tip. Delete the guard and the same line panics. That one-liner is the whole content of this sprint.

## Retro

- Costliest: a ten-line test consumed a worker run, a review, and four gate cycles, and most of the review's budget went to one question — whether a panic-as-pin is acceptable. That question was worth answering once, in writing, because it will come again with every unique-plugin guard; it is the kind of finding that belongs in `AGENTS.md`, which this loop may not edit.
- Blocked: nothing. The mirror's mtime hazard is now routine rather than novel: this cycle the worker touched the repo's test file before running the gate specifically to avoid replaying a stale binary, which is the procedure cycle 3's false red produced.
- Skipped or retried: none, and no item text needed correcting this cycle — the only discrepancy was the scout's own report claiming it had not written `SPRINT-TODO.md` when the diff showed it had. Content was sound, so it was landed as written; the lesson is to trust `git diff` over a child's self-report.
- The queue is now down to owner calls. Four scout passes produced eleven landed items (G-1, C-1..C-3, T-1..T-7) and an empty remainder; the next scout pass will very likely return nothing in scope, and the right end to this loop is a Finish with the open decisions listed rather than a fourth invented item.

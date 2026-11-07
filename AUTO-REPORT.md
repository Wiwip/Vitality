# AUTO-REPORT — `/auto` run 2026-11-07, goal "execute the implementation plan"

Ended on loop budget: cycle 6 would have exceeded `max_cycles=5`. Not blocked, no `.auto/STOP`, no cancelled child. State: `cycle=6`, `phase=idle`, `finished=2026-11-07T18:58:00Z`.

Branch `auto/2026-11-07`, base `main`, tip `1694afb`. Five sprints, all judged PASS by a fresh reviewer, all ff-merged. 13 items landed, 0 skipped, 0 parked mid-flight, 1 review BLOCK (fixed in one pass).

## What landed

| cycle | commits | verdict |
| --- | --- | --- |
| 1 | `dcd1935` todo, `992831b` G-1 library migration, `5be0992` C-1 examples BSN, `e8d98d7` C-2 `add_state_machine` + guarded `ScenePlugin`, `281750f` C-3 headless ability tests, `…summary` | `verdict-2026-11-07.md` |
| 2 | `05c19d8` todo, `e30d515` T-1 fmt, `bc0911c` T-2 migration lints, `f98dc72` T-3 pre-update grant test, `0d4667b` T-4 loud `is_in_state`, `91a7485` summary | `verdict-2026-11-07-2.md` |
| 3 | `f49d7e2` todo, `b65d457` T-5 scene-guard test, `fcbc69f` T-6 grant-chain test, `55653ca` summary | `verdict-2026-11-07-3.md` |
| 4 | `7766dcf` todo, `a08f8b7` T-7 duplicate-add guard test, `425d514` summary, `deffa8b`/`6220125` doc corrections + verdict | `verdict-2026-11-07-4.md` |
| 5 | `deffa8b` todo (+cycle-4 corrections), `c102e0e` T-8 doctests, `9b19ab2` T-9 cooldown length, `5bb32f7` summary, `1694afb` verdict | `verdict-2026-11-07-5.md` |

State at the tip: 11 integration/unit tests green, 2 doctests green, `cargo fmt --all -- --check` exit 0, `cargo check --offline --all-targets` exit 0 (warnings only in `benches/bench_systems.rs`), `cargo clippy --offline --all-targets` exit 0.

## What the plan asked for and did not get

- **Stage 7's manual example run never happened.** `examples/simple.rs` and `examples/abilities.rs` compile and are pinned as compile-only (decision D-7); there is no display in this sandbox, so nobody has run them since the migration. That is the one plan gate still open.
- **Four judged sprints coexisted with a red documented check.** Decision D-2 scoped this loop's gate to `cargo test --lib --tests`, and neither `check --all-targets` nor clippy covers doctests, so two non-compiling `EffectBuilder` examples sat through cycles 1-4 undetected. `AGENTS.md` documents the build loop as plain `cargo test --offline`. Fixed in cycle 5 (T-8), but the blind spot itself is still there, because `scripts/gate.sh` is a file this loop would not edit.

## Do these, in this order

1. **Commit `Cargo.toml` with the branch.** It carries the bevy `0.19` → `0.20.1` bump and `hfsm_bevy = { path = "../hfsm-rs/crates/hfsm_bevy" }`, and `land.sh` never stages it because it is listed in `.git/sprint-foreign`. Nothing on `auto/2026-11-07` compiles from a clean checkout without it, and `main` cannot build at all on its own manifest (it points at `path = "../hfsm_bevy"`, which does not exist).
2. **Add `--doc` to `scripts/gate.sh`** so the T-8 class cannot rot again. Note that doctests only execute here with `LD_LIBRARY_PATH=$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/lib`, because `Cargo.toml:37` pulls bevy with `dynamic_linking`; without it the runner dies on a missing `libstd-…so` while still printing compile errors, which is easy to misread as a pass.
3. **Add a `touch` after `gate.sh`'s `tar`** (or use `tar --touch`). The mirror restores mtimes, so cargo can replay a stale binary. That produced one false red landing in cycle 3 (four ability tests failed on a clean tree), one false green risk in cycle 4, and one bogus failure during cycle 5's re-review. Every child here therefore only trusted runs that printed `Compiling vitality`.
4. **Fix the four record corrections** in `verdict-2026-11-07-5.md` (three prose lines in `SPRINT-LOG.md`/`SPRINT-TODO.md`, one assert message that cites `src/lib.rs:102` where the code is `:103`). They are owed by me, not by the work.
5. **Answer the open design decisions**, listed in `.auto/decisions.md` as `assumed` or `parked`: whether `AttributesPlugin` should guarantee fixed ticks for headless apps (D-9, tests install `TimeUpdateStrategy` themselves today), the public-API policy for hfsm's `Result` returns (D-3: `is_in_state` tolerates as `false` and now logs, `dispatch_event` logs and continues), whether a gameplay plugin should own `ScenePlugin` install at all (D-10), whether the mirror gate survives as the project's gate (D-1), and whether non-Bevy dependency bumps are wanted (D-4, parked).

## Owner-call defects, unchanged and unpinned

- Nested `Tasks` never activate downward: `ActiveState::on_enter` dispatches `TaskEvent::Activate` only to direct children (`src/ability/ability_state.rs:216-224`), while `examples/abilities.rs:64-79` builds exactly that nested shape.
- `Vitality::get_global_actor()` `.single().unwrap()` (`src/context.rs:196`) panics before the first `app.update()`; it forced a stand-in-and-despawn dance into three different tests.
- `init_asset::<ScenePatch>()` overwrites, so a host that loads scene patches before adding vitality loses them (`bevy_asset-0.20.1/src/lib.rs:656-673`); the test asserts existence, not content.
- Every vitality plugin is unique and `AttributesPlugin` already adds `AbilityPlugin`, so adding both panics; adding `AttributesPlugin` before `DefaultPlugins` now claims the scene install.
- `src/effect/builder.rs:59-60` and `src/registry/actor_registry.rs:10,15` carry four unresolved intra-doc links, and that stale vocabulary is what produced the non-compiling example T-8 fixed. `src/effect/builder.rs:110` adds Health in an example whose prose says damage over time.
- `benches/bench_systems.rs` bodies are commented out, so `cargo bench` measures nothing and emits 15 dead-code warnings. `src/inspector/debug_overlay.rs:34` needs `assets/fonts/*.ttf`, which are git-ignored.

## Honesty of this run's own record

- Verdicts are written by the same model that authored the work; each one says so at the top. A second model never reviewed anything.
- Controller-authored claims were disproved five times and the corrections are all in the verdict files: the `init_asset` insert-if-missing error (verdict 3), the `Compiling vitality` build-line claims (verdicts 4 and 5), `ten landed items` for eleven, the "protected from worker edits" characterisation, and a panic column. Two were also fixed in the docs at the next landing (`deffa8b`); the rest are listed above because no landing remained.
- Item text was wrong three times and each was handled at landing rather than quietly re-scoped: T-2's unreachable `grep == 0`, T-3's "no `GlobalActor` needed", T-9's half-advance recipe. The T-9 correction note failed to apply on the first attempt, so `SPRINT-TODO.md` still carries the disproved recipe (verdict 5 finding 2).
- No cost data exists: `~/.pi/agent/sprint/stats.sh` was removed mid-run (the sprint tooling was replaced at about 12:45, which also added `close-branch.sh`) and `history.tsv` is read-only in this sandbox regardless. `scripts/size.sh` never existed here.
- The owner's foreign work was preserved throughout: `Cargo.toml` and `src/systems.rs` are still uncommitted and unstaged, and no commit on `auto/2026-11-07` touches them, `.cargo/**`, `rust-toolchain.toml`, `.agent-sandbox`, `.idea/**`, `AGENTS.md` or `scripts/**`.
- Nothing was pushed. Two `ask_user` batches early in the run were dismissed, so every later judgement call was taken as `assumed` and recorded in `.auto/decisions.md` rather than asked again.

## What the queue still needs

Nothing that this loop may act on. All 13 items in `SPRINT-TODO.md` are `status: done` and cycle 5's scout pass drained the in-scope work; the two items it still lists as blocked are blocked by files this loop may not edit (`scripts/gate.sh`) or by foreign state (`Cargo.toml`). The one genuinely in-scope thing named after the last landing is the stale doc prose in `src/effect/builder.rs:59-60`, which is item 5 above.

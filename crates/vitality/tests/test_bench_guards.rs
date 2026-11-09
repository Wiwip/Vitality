//! B-5: the bench post-loop guards must be filter-safe, and the rule that keeps them so has to be
//! gate-visible for the same reason `tests/test_bench_wiring.rs` is — `scripts/gate.sh` runs
//! `cargo test --workspace --lib --tests`, which never builds a bench target, so a guard that panics
//! (or silently passes) on a filtered `cargo bench -- <filter>` run stays invisible until someone
//! notices the panic.
//!
//! Why a *new* file instead of a rule 4 in `test_bench_wiring.rs`: the wiring guard walks directory
//! entries and path attributes and compiles no behaviour at all, while the half of this file that
//! pins the helper's semantics needs `support/guard.rs` as a real module — keeping the two apart
//! means the text-only guard cannot be broken by a change to the helper, and vice versa.
//!
//! ## The rule, stated plainly
//!
//! In `benches/bench_systems.rs`, inside any `fn` body that calls a `Bencher` method (a call written
//! `b.iter…(` or `bencher.iter…(`), every statement that *starts* with an assert-ish name — an
//! identifier containing `assert`, bare (`assert!`, `assert_eq!`) or as the last segment of a path
//! (`recompute::assert_dirty_attrs(`) — that sits after the first such bench call must be inside the
//! text region of one of the two sanctioned idioms:
//!
//! * the bench call itself, i.e. the assert rides inside the closure, which is what
//!   `harness/app_update` (in its `iter_batched_ref` setup) and the `leaf/*` groups do; or
//! * a `.guard(` call, i.e. [`guard::Ran`], which is what every post-loop assert in the file does
//!   since B-5.
//!
//! An assert *before* the first bench call of its body is exempt: it reads state the group function
//! built itself, so it is true under every filter (the pre-loop `attrs().len()` and `shape.ticking`
//! asserts).
//!
//! ## What the rule cannot see
//!
//! It reads text, so it misses: an assert that does not have `assert` in its name (`panic!`,
//! `unreachable!()`, a helper called `check_shape`) or one a macro expands into, and anything inside
//! a `macro_rules!` body or inside `benches/support/*.rs` — a support helper is covered only through
//! the name of its call site, which is why `assert_dirty_attrs` counts. It cannot see a *deleted*
//! guard, only the `guard_regions` tripwire below notices the drop. It assumes the `|b|` receiver
//! naming this file uses everywhere: a bencher bound to another name reads as "no bench loop here",
//! which fails in the safe direction (an in-closure assert then gets flagged, a guard is never
//! waived). And it cannot tell whether the condition a guard asserts is the one its id claims — only
//! that a filter cannot reach it unguarded.

#[path = "../benches/support/guard.rs"]
mod guard;

use guard::Ran;
use std::path::PathBuf;

/// What the scan of one source found: the violations, plus the counts that prove the scanner looked
/// at something (a rule that matches nothing passes as loudly as one that bites).
struct Audit {
    violations: Vec<String>,
    /// `fn` bodies containing at least one `Bencher` call.
    benched_bodies: usize,
    /// asserts after the first bench call of a benched body, sanctioned or not.
    post_loop_asserts: usize,
    /// regions opened by a `.guard(` call.
    guard_regions: usize,
}

/// Inclusive 1-based line range.
#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    end: usize,
}

impl Span {
    fn holds(&self, line: usize) -> bool {
        self.start <= line && line <= self.end
    }
}

fn bench_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("bench_systems.rs")
}

fn read(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("bench guard cannot read {}: {err}", path.display());
    })
}

/// A line with its `//` tail gone, so a doc comment that mentions `b.iter` or an assert is not code.
/// No string literal in the files scanned here contains `//`.
fn code(line: &str) -> String {
    match line.find("//") {
        Some(cut) => line[..cut].to_string(),
        None => line.to_string(),
    }
}

/// Bracket balance a line contributes, counting `()[]{}` together so a call region ends at the
/// closing bracket of the call. Braces inside format strings come in balanced pairs.
fn delta(code: &str) -> i32 {
    let mut depth = 0;
    for ch in code.chars() {
        match ch {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn depth_before(lines: &[String]) -> Vec<i32> {
    let mut depths = Vec::with_capacity(lines.len());
    let mut depth = 0;
    for line in lines {
        depths.push(depth);
        depth += delta(line);
    }
    depths
}

/// The region of every call whose head line `is_head` accepts: from that line to the line whose
/// running depth first returns to the depth the head line started at.
fn call_spans(lines: &[String], depths: &[i32], is_head: &dyn Fn(&str) -> bool) -> Vec<Span> {
    let mut spans = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !is_head(line) {
            continue;
        }
        let base = depths[index];
        let mut end = lines.len() - 1;
        for candidate in index..lines.len() {
            if depths[candidate] + delta(&lines[candidate]) <= base {
                end = candidate;
                break;
            }
        }
        spans.push(Span {
            start: index + 1,
            end: end + 1,
        });
    }
    spans
}

/// `.iter(` / `.iter_batched_ref(` … on the `Bencher` binding, i.e. a timed loop rather than an
/// iterator chain. This file names the parameter `b` in every bench closure.
fn is_bench_call(line: &str) -> bool {
    for receiver in ["b.", "bencher."] {
        let mut hay = line;
        while let Some(at) = hay.find(receiver) {
            let prior = hay[..at].chars().next_back();
            let rest = &hay[at + receiver.len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let plain_receiver = !prior.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            if plain_receiver && name.starts_with("iter") && rest[name.len()..].starts_with('(') {
                return true;
            }
            hay = &hay[at + receiver.len()..];
        }
    }
    false
}

fn is_guard_call(line: &str) -> bool {
    line.contains(".guard(")
}

fn is_bump(line: &str) -> bool {
    line.contains(".bump(")
}

/// The trimmed text of a line that *starts* with an assert-ish call: a bare `assert_eq!(` or a path
/// whose last segment contains `assert`, so a helper like `assert_dirty_attrs(` counts and a format
/// string that merely mentions an assert does not.
fn assert_head(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let mut head = trimmed;
    while let Some((_, tail)) = head.split_once("::") {
        head = tail;
    }
    let ident: String = head
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!ident.is_empty() && ident.contains("assert"))
        .then(|| format!("{} ({ident})", trimmed.trim_end()))
}

/// Top-level `fn` bodies: the first `{` at or after the `fn` line, to the line that closes it again.
/// Braces only, because a signature spread over lines closes its own parentheses long before its
/// body opens (`fn cross_actor_point<S, D>(\n … \n) where`) and counting those ends the body early.
fn fn_bodies(lines: &[String]) -> Vec<(String, Span)> {
    let mut bodies = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(name) = fn_name(&lines[index]) else {
            index += 1;
            continue;
        };
        let Some(open) = (index..lines.len()).find(|line| lines[*line].contains('{')) else {
            break;
        };
        let mut braces = 0;
        let mut end = lines.len() - 1;
        for candidate in open..lines.len() {
            braces += lines[candidate].matches('{').count() as i32
                - lines[candidate].matches('}').count() as i32;
            if braces == 0 {
                end = candidate;
                break;
            }
        }
        bodies.push((
            name,
            Span {
                start: index + 1,
                end: end + 1,
            },
        ));
        index = end + 1;
    }
    bodies
}

fn fn_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("pub(crate) ")
        .or_else(|| trimmed.strip_prefix("pub "))
        .unwrap_or(trimmed);
    let rest = rest.strip_prefix("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

fn audit(source: &str) -> Audit {
    let lines: Vec<String> = source.lines().map(|line| code(line)).collect();
    let depths = depth_before(&lines);
    let benches = call_spans(&lines, &depths, &is_bench_call);
    let guards = call_spans(&lines, &depths, &is_guard_call);

    let mut violations = Vec::new();
    let mut benched_bodies = 0;
    let mut post_loop_asserts = 0;

    for (name, body) in fn_bodies(&lines) {
        let body_benches: Vec<Span> = benches
            .iter()
            .filter(|span| body.holds(span.start))
            .copied()
            .collect();
        if body_benches.is_empty() {
            continue;
        }
        benched_bodies += 1;
        let first = body_benches.iter().map(|span| span.start).min().unwrap();
        let body_guards: Vec<Span> = guards
            .iter()
            .filter(|span| body.holds(span.start))
            .copied()
            .collect();

        for line in body.start..=body.end {
            let Some(text) = assert_head(&lines[line - 1]) else {
                continue;
            };
            if line <= first {
                continue;
            }
            post_loop_asserts += 1;
            let sanctioned = body_benches.iter().any(|span| span.holds(line))
                || body_guards.iter().any(|span| span.holds(line));
            if !sanctioned {
                violations.push(format!("fn {name}, line {line}: {text}"));
            }
        }
    }

    Audit {
        violations,
        benched_bodies,
        post_loop_asserts,
        guard_regions: guards.len(),
    }
}

/// Rule 1: no bare assert sits between a timed loop and the end of its body.
#[test]
fn every_post_loop_bench_assert_goes_through_the_guard_helper() {
    let source = read(&bench_file());
    let found = audit(&source);

    assert!(
        found.violations.is_empty(),
        "post-loop assert(s) in {} that a criterion filter can reach without its loop ever \
         running: {:#?}\n\
         a filter skips the closure but not the code around it (criterion 0.8.2 \
         benchmark_group.rs:303-324), so such an assert either panics on state the run never built \
         or passes without having looked at anything. Use one of the two sanctioned forms: assert \
         inside the closure (`app_update`, `leaf/*`) or after a `guard::Ran` bump inside the closure \
         (`benches/support/guard.rs`).",
        bench_file().display(),
        found.violations,
    );
    assert!(
        found.benched_bodies >= 6,
        "the guard scanned {} `fn` bodies with a timed loop in {}; a scan that sees no bench call \
         would pass this rule without checking anything",
        found.benched_bodies,
        bench_file().display(),
    );
    assert!(
        found.post_loop_asserts >= 7,
        "the guard examined {} asserts after a timed loop; B-5 converted seven (floor/idle_tick, \
         recompute/*, recompute/cross_actor, two in periodic/tick_scan, two in allocs_per_tick), so \
         a scan that finds fewer is not reading the file",
        found.post_loop_asserts,
    );
    assert!(
        found.guard_regions >= 5,
        "the guard found {} `.guard(` regions; the six B-5 sites need at least five (the two \
         periodic and the two allocs_per_tick asserts each share one), so a dropped guard call is \
         the bench losing a check the gate cannot otherwise notice",
        found.guard_regions,
    );
}

/// Rule 2: the ran-flag a guard consults has to be bumped by the loop it guards, and only from
/// inside that loop — a `guard` whose bump was left out skips forever and reads as an honest run.
#[test]
fn every_guard_is_bumped_from_inside_its_own_timed_loop() {
    let source = read(&bench_file());
    let lines: Vec<String> = source.lines().map(|line| code(line)).collect();
    let depths = depth_before(&lines);
    let benches = call_spans(&lines, &depths, &is_bench_call);

    let mut problems: Vec<String> = Vec::new();
    for (name, body) in fn_bodies(&lines) {
        let guarded = (body.start..=body.end)
            .filter(|line| is_guard_call(&lines[line - 1]))
            .count();
        if guarded == 0 {
            continue;
        }
        let bumps: Vec<usize> = (body.start..=body.end)
            .filter(|line| is_bump(&lines[line - 1]))
            .collect();
        if bumps.len() < guarded {
            problems.push(format!(
                "fn {name}: {guarded} `.guard(` call(s) but {} bump(s), so a guard can report a \
                 loop that never ran",
                bumps.len()
            ));
        }
        for line in bumps {
            let inside = benches
                .iter()
                .filter(|span| body.holds(span.start))
                .any(|span| span.holds(line));
            if !inside {
                problems.push(format!(
                    "fn {name}, line {line}: bump outside a timed loop, so it counts even when the \
                     filter skips the closure"
                ));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "the ran-flag of benches/support/guard.rs is only honest while the timed closure bumps it: \
         {problems:#?}",
    );
}

/// The rule's bite, on a source this file owns: an assert after a `bencher.iter` loop — bare, or
/// tucked inside an iterator `for` loop — must be reported, and both sanctioned idioms must not be.
/// Without this the real-file rule above could rot into a scan that matches nothing.
#[test]
fn the_rule_bites_a_bare_assert_after_a_timed_loop() {
    let bare = r#"
fn idle(c: &mut Criterion) {
    let ran = guard::Ran::new();
    group.bench_function("x", |b| {
        b.iter(|| {
            ran.bump();
            run_fixed_tick(world);
        });
    });
    assert!(true);
}

fn cascade(c: &mut Criterion) {
    group.bench_function("y", |b| {
        b.iter(|| tick(world));
    });
    for (reader, was) in readers.iter().zip(before) {
        assert!(value(reader) > was);
    }
}
"#;
    let violations = audit(bare).violations;
    assert_eq!(
        violations.len(),
        2,
        "a bare post-loop assert and one hidden in an iterator loop must both read as violations, \
         found {violations:#?}",
    );

    let sanctioned = r#"
fn in_the_closure(c: &mut Criterion) {
    group.bench_function("y", |b| {
        b.iter_batched_ref(
            || {
                assert_eq!(marked(), 25);
                build()
            },
            tick,
            BatchSize::PerIteration,
        );
    });
}

fn behind_a_guard(c: &mut Criterion) {
    let ran = guard::Ran::new();
    group.bench_function("x", |b| {
        b.iter(|| {
            ran.bump();
            run_fixed_tick(world);
        });
    });
    ran.guard("x", || {
        assert_eq!(dirty_entity_count(world), 0);
    });
}

fn before_the_loop(c: &mut Criterion) {
    assert_eq!(attrs(), 8);
    group.bench_function("z", |b| {
        b.iter(|| tick(world));
    });
}
"#;
    assert_eq!(
        audit(sanctioned).violations,
        Vec::<String>::new(),
        "assert inside the closure, behind a `.guard(` and before the loop are all filter-safe",
    );
}

/// The helper's own contract, which is what makes a filtered run honest: no iterations means no
/// assert and a printed reason; any iteration means the assert runs and nothing extra is printed.
/// Pinning this here is also what makes a helper whose flag were "always ran" go red in the gate.
#[test]
fn a_guard_only_asserts_on_a_loop_that_actually_ran() {
    let filtered = Ran::new();
    let mut checked = false;
    let guard_ran = filtered.guard("floor/idle_tick/a=1/run_fixed_tick", || checked = true);
    assert!(
        !guard_ran && !checked,
        "a guard whose benchmark was filtered out must not assert: it would be reading a world the \
         timed loop never touched ({} iterations recorded)",
        filtered.iterations(),
    );
    assert!(!filtered.ran(), "no bump means the loop never ran");

    let timed = Ran::new();
    for _ in 0..10 {
        timed.bump();
    }
    let mut checked = false;
    assert!(
        timed.guard("floor/idle_tick/a=1/run_fixed_tick", || checked = true) && checked,
        "a guard whose loop ran must run its assert, or B-5 has replaced seven checks with skip \
         lines ({} iterations recorded)",
        timed.iterations(),
    );

    assert_eq!(
        guard::skip_line("floor/idle_tick/a=1/run_fixed_tick"),
        "guard skipped (floor/idle_tick/a=1/run_fixed_tick filtered out)",
        "the reason a filtered guard did not run has to name the id it skipped, in one line, so a \
         filtered bench run says what it did not check",
    );
}

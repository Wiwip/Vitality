//! B-5: the bench post-loop guards must be filter-safe, and the rule that keeps them so has to be
//! gate-visible for the same reason `tests/test_bench_wiring.rs` is — `scripts/gate.sh` runs
//! `cargo test --workspace --lib --tests`, which never builds a bench target, so a guard that panics
//! (or silently passes) on a filtered `cargo bench -- <filter>` run stays invisible until someone
//! notices the panic.
//!
//! Why a *new* file instead of a rule in `test_bench_wiring.rs`: the wiring guard walks directory
//! entries and path attributes and compiles no behaviour at all, while the half of this file that
//! pins the helper's semantics needs `support/guard.rs` as a real module — keeping the two apart
//! means the text-only guard cannot be broken by a change to the helper, and vice versa.
//!
//! ## The rule, stated plainly
//!
//! In `benches/bench_systems.rs`, inside any `fn` body that calls a `Bencher` method (a call written
//! `b.iter…(` or `bencher.iter…(`), every statement line after the first such bench call that *can
//! abort or read state* must sit inside the text region of one of the two sanctioned idioms. A line
//! can abort or read state when one of its identifiers contains `assert`, when it carries one of the
//! panicking macros (`panic!`, `unreachable!`, `todo!`, `unimplemented!`), or when it calls a helper
//! whose own body does either — see [`checking_fns`]. The two sanctioned idioms are:
//!
//! * the benchmark registration itself — the text of a `bench_function(…)` call, which covers both
//!   the timed closure and the world or id built for that one benchmark, because criterion never
//!   calls the closure of a benchmark its filter dropped; or
//! * a `.guard(` call, i.e. [`guard::Ran`], which is what every post-loop assert in the file does
//!   since B-5.
//!
//! An assert *before* the first bench call of its body is exempt: it reads state the group function
//! built itself, so it is true under every filter (the pre-loop `attrs().len()` and `shape.ticking`
//! asserts).
//!
//! Three tightenings, one per hole B-6 found in the rule as B-5 left it, each pinned by its own test
//! below. Every one of them is proved red on a *synthetic* source handed to [`audit`] — no test edits
//! or copies `bench_systems.rs`: hole 2 is
//! [`a_post_loop_panic_reads_as_a_check_that_needs_a_guard`], hole 3 is
//! [`a_post_loop_call_to_a_helper_that_asserts_inside_reads_as_a_violation`], hole 1 is the
//! `names_a_path` body inside [`the_rule_bites_a_bare_assert_after_a_timed_loop`], and the group rename
//! of rule 4 is [`a_stale_guard_id_reads_as_a_lie_about_its_group`]. Each of those four also carries the
//! green half (the same shape behind a `.guard(`, or the honest id) so a rule cannot pass by matching
//! nothing. The real file is what the three `every_*` tests read as it stands —
//! [`every_post_loop_bench_assert_goes_through_the_guard_helper`],
//! [`every_guard_is_bumped_from_inside_its_own_timed_loop`] and
//! [`every_guard_id_names_a_group_the_bench_file_opens`] — so those go red only on a violation the bench
//! file actually contains, never on one a test imagined:
//!
//! * the observable name is read off **every identifier of the line**, never off a chosen segment of
//!   it. The first version split the whole line on `::` and kept the last segment, so
//!   `assert!(identity::distinguishable(w) == 3)` read as a call to `w`; C-1 closed that by cutting at
//!   the call's own `(`, and reading identifiers makes the whole class unrepresentable (hole 1).
//! * a panicking macro counts, so `if dirty > 0 { panic!("…") }` after a timed loop is a violation
//!   like the bare assert it is equivalent to (hole 2).
//! * a helper *called* after the loop is judged by its body, not by its name: the scan reads
//!   `benches/support/*.rs` too, so `check_shape(world)` — a helper that asserts without saying so in
//!   its name, which is the shape `identity.rs`'s `assert_distinguishable` takes the first time a
//!   group calls it from outside a guard — is caught as `assert_dirty_attrs` already was, and so is
//!   the same call hidden behind `let ok = check_shape(world);` (hole 3).
//!
//! Rule 4 pins the other half of a guard's honesty: the id a guard prints must carry the name of the
//! group its own body opens (or of the file-level `const` a group is opened with), so renaming a
//! `benchmark_group("…")` cannot leave `guard skipped (<id> filtered out)` naming an id that no longer
//! exists. The rule reads ids that *claim* a group: an id with no `/` in it is not a criterion id, and
//! [`guard_id_problems`] exempts it — which is where `RATIO_TABLE = "recompute ratio table"` lives,
//! because the ratio table is printed by the run and never registered with criterion, so no group rename
//! can make its skip line lie. That exemption is pinned rather than assumed:
//! [`every_guard_id_names_a_group_the_bench_file_opens`] runs rule 4 over the real file and counts that
//! guard among the ids it read (`checked >= 5`), so closing the exemption without giving the table a
//! slashed id is a red test, not a quiet one.
//!
//! ## What the rule cannot see
//!
//! It reads text, so it misses: an assert a macro expands into, one that only *starts* inside another
//! macro's argument list (`warn!(assert_eq!(a, b))` reads as a call to `warn`), and anything inside a
//! `macro_rules!` body. A helper reached through a value the scan cannot resolve (`rows.iter().all(check)`,
//! a trait method, a `dyn Fn` a guard was handed) reads as clean, and so does `values.first().unwrap()`
//! after the loop: `expect`/`unwrap` are deliberately not abort names, because half the support
//! helpers use them for their own setup and the set would say nothing about the checks that matter.
//! Helper names are matched without scope, so a local named `line` or `probe` is a call to the
//! support function of that name only when the call position says so, and a qualified call is checked
//! against the module it names — an unmapped qualifier falls back to the bare name, which over-reports
//! rather than passing silently. It cannot see a *deleted* guard either; only the `guard_regions`
//! tripwire notices that drop. It assumes the `|b|` receiver naming this file uses everywhere: a
//! bencher bound to another name reads as "no bench loop here", which fails in the safe direction (an
//! in-closure assert then gets flagged, a guard is never waived). It reads identifiers, not syntax, so
//! a line whose *string* says "assert" is read as an assert — that direction fails as a violation
//! someone fixes, never as a pass nobody notices. And it cannot tell whether the condition a guard
//! asserts is the one its id claims: rule 4 compares an id with the group names the file declares,
//! nothing more.

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
    /// statements after the first bench call of a benched body that can abort or read state,
    /// sanctioned or not.
    post_loop_checks: usize,
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

/// A source paired with the module name calls qualify themselves with: `support/leaf.rs` is what
/// `leaf_fixture` and `leaf::leaf_fixture` are both read against.
type Source = (String, String);

fn bench_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("bench_systems.rs")
}

/// The helpers a post-loop call can hide an abort inside. The rule judges `bench_systems.rs`, but a
/// guard's assert usually lives in one of these files, so their bodies decide which names a post-loop
/// call must not reach unguarded.
fn support_sources() -> Vec<Source> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("support");
    let mut sources: Vec<Source> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("bench guard cannot read {}: {err}", dir.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "rs").then_some(path)
        })
        .map(|path| {
            let stem = path
                .file_stem()
                .expect("a .rs file has a stem")
                .to_string_lossy()
                .to_string();
            let text = read(&path);
            (stem, text)
        })
        .collect();
    sources.sort();
    sources
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

/// One benchmark's own registration. Criterion reads the filter before it calls the closure, so
/// everything inside this call — the id it builds, the world its setup makes, the asserts that ride
/// along — only exists on a run that timed that benchmark.
fn is_registration(line: &str) -> bool {
    line.contains("bench_function(")
}

fn is_guard_call(line: &str) -> bool {
    line.contains(".guard(")
}

fn is_bump(line: &str) -> bool {
    line.contains(".bump(")
}

/// The macros that abort a run whatever state they read.
const PANIC_MACROS: [&str; 4] = ["panic", "unreachable", "todo", "unimplemented"];

/// The identifier-shaped runs of a line, so an assert inside a path, a `let` or an `if` is still read
/// as the name it is. B-5 read the last `::` segment of the whole line instead, which turned
/// `assert!(identity::distinguishable(w) == 3)` into a call to `w`.
fn identifiers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// One callee written on a line: its name, and the path segment in front of it when the call is
/// qualified. `alloc_count::line(…)` and `excl_fixture::<Excl0>(…)` are calls; `probe.excl` and a
/// plain field read are not, which is what keeps a local named after a helper from reading as one.
fn calls(text: &str) -> Vec<(Option<String>, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut previous: Option<String> = None;
    let mut pending: Option<(String, Option<String>)> = None;
    let mut generics: i32 = 0;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_alphanumeric() || ch == '_' {
            let start = index;
            while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            let name: String = chars[start..index].iter().collect();
            // Inside `<…>` the names are arguments, not the callee, so `f::<T>(x)` is still a call to
            // `f` and `fn read_excl<B: Bundle>(…)` still one to `read_excl`.
            if generics == 0 {
                let head: String = chars[..start].iter().collect();
                let qualifier = if head.trim_end().ends_with("::") {
                    previous.clone()
                } else {
                    None
                };
                pending = Some((name.clone(), qualifier));
            }
            previous = Some(name);
            continue;
        }
        if ch == '(' {
            if let Some((name, qualifier)) = pending.take() {
                out.push((qualifier, name));
            }
        } else if ch == '<' {
            generics += 1;
        } else if ch == '>' {
            generics = generics.saturating_sub(1);
        } else if !(ch.is_whitespace()
            || matches!(ch, ':' | '!' | '?')
            || (ch == ',' && generics > 0))
        {
            pending = None;
        }
        index += 1;
    }
    out
}

/// A function the scan knows can abort on the state it is handed: which module defines it, and its
/// name. A post-loop call is judged against this, so a helper no longer gets away by being named
/// `check_shape` instead of `assert_shape`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Checker {
    module: String,
    name: String,
}

/// Why this statement cannot sit after a timed loop unguarded: its own name asserts, it is a
/// panicking macro, or it calls one of `checkers`.
fn observable(line: &str, checkers: &[Checker], modules: &[String]) -> Option<String> {
    let trimmed = line.trim();
    let names = identifiers(trimmed);
    if let Some(name) = names.iter().find(|name| name.contains("assert")) {
        return Some(format!("`{name}` in `{trimmed}`"));
    }
    if let Some(macro_name) = PANIC_MACROS
        .iter()
        .find(|macro_name| trimmed.contains(&format!("{macro_name}!")))
    {
        return Some(format!("`{macro_name}!` in `{trimmed}`"));
    }
    calls(trimmed).into_iter().find_map(|(qualifier, name)| {
        let checker = checkers.iter().find(|checker| {
            checker.name == name
                && qualifier
                    .as_ref()
                    .is_none_or(|path| !modules.contains(path) || checker.module == *path)
        })?;
        Some(format!(
            "`{}::{}`, which asserts inside, in `{trimmed}`",
            checker.module, checker.name,
        ))
    })
}

/// Every function in these sources that can abort on state it is handed: its body carries an assert, a
/// panicking macro or — transitively, to a fixed point — a call to another such function, anywhere
/// outside its own `.guard(` region.
fn checking_fns(sources: &[Source]) -> Vec<Checker> {
    let parsed: Vec<(&str, Vec<String>, Vec<i32>)> = sources
        .iter()
        .map(|(module, text)| {
            let lines: Vec<String> = text.lines().map(|line| code(line)).collect();
            let depths = depth_before(&lines);
            (module.as_str(), lines, depths)
        })
        .collect();
    let modules: Vec<String> = parsed
        .iter()
        .map(|(module, _, _)| module.to_string())
        .collect();
    let mut names: Vec<Checker> = Vec::new();
    loop {
        let mut grew = false;
        for (module, lines, depths) in &parsed {
            let guards = call_spans(lines, depths, &is_guard_call);
            for (name, body) in fn_bodies(lines) {
                if names
                    .iter()
                    .any(|seen| seen.name == name && seen.module == *module)
                {
                    continue;
                }
                let body_guards: Vec<Span> = guards
                    .iter()
                    .filter(|span| body.holds(span.start))
                    .copied()
                    .collect();
                let aborts = (body.start..=body.end).any(|line| {
                    !body_guards.iter().any(|span| span.holds(line))
                        && observable(&lines[line - 1], &names, &modules).is_some()
                });
                if aborts {
                    names.push(Checker {
                        module: module.to_string(),
                        name,
                    });
                    grew = true;
                }
            }
        }
        // Bounded: a pass only repeats while it found a name it had not found before.
        if !grew {
            break;
        }
    }
    names
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
    audit_with(source, &[])
}

/// Rule 1 over one source, with `helpers` (the bench support files, in the real scan) contributing
/// the names a post-loop call can abort through.
fn audit_with(source: &str, helpers: &[Source]) -> Audit {
    let mut sources: Vec<Source> = vec![("bench_systems".to_string(), source.to_string())];
    sources.extend(helpers.iter().cloned());
    let modules: Vec<String> = sources.iter().map(|(module, _)| module.clone()).collect();
    let checkers = checking_fns(&sources);

    let lines: Vec<String> = source.lines().map(|line| code(line)).collect();
    let depths = depth_before(&lines);
    let benches = call_spans(&lines, &depths, &is_bench_call);
    let registrations = call_spans(&lines, &depths, &is_registration);
    let guards = call_spans(&lines, &depths, &is_guard_call);

    let mut violations = Vec::new();
    let mut benched_bodies = 0;
    let mut post_loop_checks = 0;

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
        let sanctioned: Vec<Span> = guards
            .iter()
            .chain(registrations.iter())
            .filter(|span| body.holds(span.start))
            .copied()
            .collect();

        for line in body.start..=body.end {
            if line <= first {
                continue;
            }
            let Some(text) = observable(&lines[line - 1], &checkers, &modules) else {
                continue;
            };
            post_loop_checks += 1;
            let inside = benches.iter().any(|span| span.holds(line))
                || sanctioned.iter().any(|span| span.holds(line));
            if !inside {
                violations.push(format!("fn {name}, line {line}: {text}"));
            }
        }
    }

    Audit {
        violations,
        benched_bodies,
        post_loop_checks,
        guard_regions: guards.len(),
    }
}

/// The text after the first `needle`, or nothing when the line does not carry it.
fn after<'a>(text: &'a str, needle: &str) -> Option<&'a str> {
    text.find(needle).map(|at| &text[at + needle.len()..])
}

/// The inside of a `"…"` written at the start of `text`.
fn string_literal(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.to_string())
}

/// The first argument of a call whose text starts at `rest`: up to the first `,` outside brackets.
fn first_argument(rest: &str) -> Option<String> {
    let mut depth = 0;
    let mut out = String::new();
    for ch in rest.chars() {
        match ch {
            '(' | '[' | '{' => {
                depth += 1;
                out.push(ch);
            }
            ')' | ']' | '}' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                out.push(ch);
            }
            ',' if depth == 0 => break,
            _ => out.push(ch),
        }
    }
    Some(out)
}

/// A `let NAME = "…";` or `const NAME: &str = "…";` line, as (name, value, is_const): how a group
/// name written once and passed to `benchmark_group` by name is read back, and whether every body in
/// the file can see it.
fn string_binding(line: &str) -> Option<(String, String, bool)> {
    let trimmed = line.trim();
    let is_const = trimmed.starts_with("const ");
    let rest = trimmed
        .strip_prefix("let ")
        .or_else(|| trimmed.strip_prefix("const "))?;
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        return None;
    }
    let (_, rhs) = rest.split_once('=')?;
    let literal = string_literal(rhs.split(';').next()?)?;
    Some((name, literal, is_const))
}

/// What a `.guard(` call claims its id is: the name it interpolates in front of the first `/`, the id
/// text it wrote out (or reached through a `const`), or nothing this scan can read.
enum IdClaim {
    /// `format!("{group}/…")`: the id starts with whatever `name` holds.
    Interpolated(String),
    /// A literal id, or a named constant resolved to its literal.
    Text(String),
    Unreadable,
}

/// The first argument of a `.guard(` call, as far as this scan can read it.
fn guard_claim(line: &str, bindings: &[(String, String, bool)]) -> IdClaim {
    let Some(rest) = after(line, ".guard(") else {
        return IdClaim::Unreadable;
    };
    let Some(arg) = first_argument(rest) else {
        return IdClaim::Unreadable;
    };
    let arg = arg.trim().trim_start_matches('&').trim().to_string();
    if let Some(format_args) = after(&arg, "format!(") {
        let Some(pattern) = first_argument(format_args.trim_start()) else {
            return IdClaim::Unreadable;
        };
        let Some(pattern) = string_literal(&pattern) else {
            return IdClaim::Unreadable;
        };
        return match pattern
            .strip_prefix('{')
            .and_then(|head| head.split('}').next())
        {
            Some(name) if !name.is_empty() => IdClaim::Interpolated(name.to_string()),
            _ => IdClaim::Unreadable,
        };
    }
    if let Some(literal) = string_literal(&arg) {
        return IdClaim::Text(literal);
    }
    let name: String = arg
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    bindings
        .iter()
        .find(|(bound, _, _)| *bound == name)
        .map_or(IdClaim::Unreadable, |(_, value, _)| {
            IdClaim::Text(value.clone())
        })
}

/// The argument of a `benchmark_group(…)` call on this line, when it carries one.
fn group_argument(line: &str) -> Option<String> {
    let rest = after(line, "benchmark_group(")?;
    Some(rest.split(')').next().unwrap_or("").trim().to_string())
}

/// Rule 4's view of the file: the names it opens groups with, and the `let`/`const` strings those
/// names may be bound to.
struct Groups {
    /// literals written straight into `benchmark_group("…")`.
    literals: Vec<String>,
    /// identifiers passed to `benchmark_group(name)`.
    names: Vec<String>,
    /// every `let`/`const` bound to a string literal, so a name can be read back as its value.
    bindings: Vec<(String, String, bool)>,
}

fn declared_groups(lines: &[String]) -> Groups {
    let mut groups = Groups {
        literals: Vec::new(),
        names: Vec::new(),
        bindings: Vec::new(),
    };
    for line in lines {
        if let Some(binding) = string_binding(line) {
            if !groups
                .bindings
                .iter()
                .any(|(name, _, _)| name == &binding.0)
            {
                groups.bindings.push(binding);
            }
        }
        let Some(argument) = group_argument(line) else {
            continue;
        };
        if let Some(literal) = string_literal(&argument) {
            if !groups.literals.contains(&literal) {
                groups.literals.push(literal);
            }
        } else if identifiers(&argument)
            .first()
            .is_some_and(|name| *name == argument)
            && !groups.names.contains(&argument)
        {
            groups.names.push(argument);
        }
    }
    groups
}

/// The group names opened inside one `fn` body. A guard may only interpolate a name *its own* body
/// passes to `benchmark_group`, or a file-level `const` that is one of the file's group names:
/// otherwise another body using the same name would hide a rename here.
fn group_names_in(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| group_argument(line))
        .filter(|argument| {
            string_literal(argument).is_none()
                && identifiers(argument)
                    .first()
                    .is_some_and(|name| name == argument)
        })
        .collect()
}

/// Every `.guard(` call in the file and whether its id still names a group the file opens. An id with
/// no `/` in it (`recompute ratio table`) is not a criterion id, so no group rename can make it lie,
/// and it is exempt; anything that does claim a group prefix has to claim a real one.
fn guard_id_problems(source: &str) -> (Vec<String>, usize, usize) {
    let lines: Vec<String> = source.lines().map(|line| code(line)).collect();
    let groups = declared_groups(&lines);
    let bodies = fn_bodies(&lines);
    let mut problems = Vec::new();
    let mut checked = 0;
    for (index, line) in lines.iter().enumerate() {
        if !is_guard_call(line) {
            continue;
        }
        checked += 1;
        let local: Vec<String> = bodies
            .iter()
            .find(|(_, span)| span.holds(index + 1))
            .map(|(_, span)| group_names_in(&lines[span.start - 1..span.end]))
            .unwrap_or_default();
        let problem = match guard_claim(line, &groups.bindings) {
            // A guard may interpolate a group name its own body opens, or a file-level `const` that
            // some body opens a group with: either way, renaming that group moves the id too.
            IdClaim::Interpolated(name) => {
                let declared_const = groups
                    .bindings
                    .iter()
                    .any(|(bound, _, is_const)| *is_const && *bound == name);
                let opened_elsewhere = groups.names.contains(&name)
                    || groups.bindings.iter().any(|(bound, value, is_const)| {
                        *is_const && *bound == name && groups.literals.contains(value)
                    });
                (!local.contains(&name) && !(declared_const && opened_elsewhere)).then(|| {
                    format!(
                        "line {}: `{{{name}}}` is not a name this body passes to `benchmark_group`, \
                         so its guard skip line can outlive a group rename",
                        index + 1,
                    )
                })
            }
            IdClaim::Text(text) => {
                let claims_a_group = text.contains('/');
                let matches = groups.literals.iter().any(|group| {
                    text.as_str() == group.as_str() || text.starts_with(&format!("{group}/"))
                });
                (claims_a_group && !matches).then(|| {
                    format!(
                        "line {}: id `{text}` does not start with any group this file opens ({:?}), \
                         so it names a benchmark that does not exist here",
                        index + 1,
                        groups.literals,
                    )
                })
            }
            IdClaim::Unreadable => Some(format!(
                "line {}: the rule cannot read this guard's id argument, so it cannot tell a skip \
                 line from a lie",
                index + 1,
            )),
        };
        if let Some(problem) = problem {
            problems.push(problem);
        }
    }
    (
        problems,
        checked,
        groups.literals.len() + groups.names.len(),
    )
}

/// Rule 1: no assert, panic or helper that asserts sits between a timed loop and the end of its body
/// without going through a guard.
#[test]
fn every_post_loop_bench_assert_goes_through_the_guard_helper() {
    let source = read(&bench_file());
    let found = audit_with(&source, &support_sources());

    assert!(
        found.violations.is_empty(),
        "post-loop check(s) in {} that a criterion filter can reach without its loop ever \
         running: {:#?}\n\
         a filter skips the closure but not the code around it (criterion 0.8.2 \
         benchmark_group.rs:303-324), so such an assert either panics on state the run never built \
         or passes without having looked at anything. Use one of the two sanctioned forms: assert \
         inside the benchmark's own `bench_function` call (`app_update`, `leaf/*`) or after a \
         `guard::Ran` bump inside the closure (`benches/support/guard.rs`).",
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
        found.post_loop_checks >= 7,
        "the guard examined {} statements after a timed loop that can abort or read state; B-5 \
         converted seven asserts (floor/idle_tick, recompute/*, recompute/cross_actor, two in \
         periodic/tick_scan, two in allocs_per_tick) and every guard body adds the reads it does, so \
         a scan that finds fewer is not reading the file",
        found.post_loop_checks,
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

/// Rule 4: a guard skip line is the only thing a filtered run prints about the check it did not do,
/// so the id in it has to be the id the group actually registers.
#[test]
fn every_guard_id_names_a_group_the_bench_file_opens() {
    let source = read(&bench_file());
    let (problems, checked, groups) = guard_id_problems(&source);

    assert!(
        problems.is_empty(),
        "guard id(s) in {} that no `benchmark_group` in that file can produce: {problems:#?}",
        bench_file().display(),
    );
    assert!(
        checked >= 5,
        "the guard read the id of {} `.guard(` calls in {}; a rule that finds no guard id cannot \
         notice a group rename",
        checked,
        bench_file().display(),
    );
    assert!(
        groups >= 6,
        "the guard found {} group names (literal or passed by name) in {}; the file opens ten \
         groups, so a scan that resolves none of them would pass this rule for every rename",
        groups,
        bench_file().display(),
    );
}

/// Rule 4 on a source this file owns: a guard whose id interpolates the name the group is opened with
/// cannot drift from it, and two ways of breaking that — renaming the group, or writing the id out by
/// hand — are both the silent lie this rule exists to catch.
#[test]
fn a_stale_guard_id_reads_as_a_lie_about_its_group() {
    let honest = r#"
fn idle(c: &mut Criterion) {
    let group_name = "floor/idle_tick";
    let mut group = c.benchmark_group(group_name);
    group.bench_function("a=1/run_fixed_tick", |b| {
        b.iter(|| {
            ran.bump();
            run_fixed_tick(world);
        });
    });
    ran.guard(&format!("{group_name}/{bench_id}"), || {
        assert_eq!(dirty_entity_count(world), 0);
    });
    group.finish();
}
"#;
    let (problems, checked, groups) = guard_id_problems(honest);
    assert_eq!(
        (problems, checked, groups),
        (Vec::new(), 1, 1),
        "a guard id built from the name the group is opened with cannot drift from it",
    );

    let renamed = honest.replace(
        "c.benchmark_group(group_name)",
        "c.benchmark_group(\"floor/slack_tick\")",
    );
    let (problems, _checked, _groups) = guard_id_problems(&renamed);
    assert_eq!(
        problems.len(),
        1,
        "renaming the group while the guard still prints its old `group_name` must be caught: \
         {problems:#?}",
    );

    let written_out = r#"
fn idle(c: &mut Criterion) {
    let mut group = c.benchmark_group("floor/idle_tick");
    group.bench_function("a=1/run_fixed_tick", |b| {
        b.iter(|| {
            ran.bump();
            run_fixed_tick(world);
        });
    });
    ran.guard("floor/slack_tick/a=1/run_fixed_tick", || {
        assert_eq!(dirty_entity_count(world), 0);
    });
    group.finish();
}
"#;
    let (problems, _checked, _groups) = guard_id_problems(written_out);
    assert_eq!(
        problems.len(),
        1,
        "a guard id written out as a literal has to carry a group this file opens: {problems:#?}",
    );
}

/// The rule's bite, on a source this file owns: an assert after a `bencher.iter` loop — bare, tucked
/// inside an iterator `for` loop, or one whose arguments name a path — must be reported, and both
/// sanctioned idioms must not be. Without this the real-file rule above could rot into a scan that
/// matches nothing.
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

fn names_a_path(c: &mut Criterion) {
    group.bench_function("w", |b| {
        b.iter(|| tick(world));
    });
    assert!(identity::distinguishable(world) == 3);
    debug_assert_eq!(recompute::dirty_names(world, pool, actor), names);
}
"#;
    let violations = audit(bare).violations;
    assert_eq!(
        violations.len(),
        4,
        "a bare post-loop assert, one hidden in an iterator loop and the two path-carrying forms \
         (`assert!(a::b() == x)`, `debug_assert_eq!(a::…, x)`) must all read as violations, found \
         {violations:#?}",
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

/// Hole 2 of B-6: the shape that is not a call to anything named `assert` but aborts just the same.
/// `if cond { panic!(…) }` after a timed loop is the bare assert with a label on it, and it used to
/// read as clean.
#[test]
fn a_post_loop_panic_reads_as_a_check_that_needs_a_guard() {
    let drifted = r#"
fn idle(c: &mut Criterion) {
    let ran = guard::Ran::new();
    group.bench_function("x", |b| {
        b.iter(|| {
            ran.bump();
            run_fixed_tick(world);
        });
    });
    if dirty_entity_count(world) > 0 {
        panic!("the timed loop left the world dirty");
    }
    unreachable!("the loop above always returns");
}
"#;
    let violations = audit(drifted).violations;
    assert_eq!(
        violations.len(),
        2,
        "a post-loop `if … {{ panic!(…) }}` and a bare `unreachable!` are checks a filter can reach \
         without its loop ever running, so both have to read as violations; found {violations:#?}",
    );

    let guarded = r#"
fn idle(c: &mut Criterion) {
    let ran = guard::Ran::new();
    group.bench_function("x", |b| {
        b.iter(|| {
            ran.bump();
            run_fixed_tick(world);
        });
    });
    ran.guard("floor/idle_tick/x", || {
        if dirty_entity_count(world) > 0 {
            panic!("the timed loop left the world dirty");
        }
    });
}
"#;
    assert_eq!(
        audit(guarded).violations,
        Vec::<String>::new(),
        "the same panic behind a `.guard(` only runs when the loop did",
    );
}

/// Hole 3 of B-6: the assert that lives one call away. Before B-6 the only thing the rule knew about a
/// helper was its name, so `assert_dirty_attrs` was visible and a helper that asserts without saying
/// so in its name — and any call at all behind a `let` — was invisible.
#[test]
fn a_post_loop_call_to_a_helper_that_asserts_inside_reads_as_a_violation() {
    let helpers = r#"
pub fn check_distinguishable(world: &World, actors: &[Entity]) {
    assert!(actors.len() > 1, "the shape needs two actors to tell apart");
}

pub fn quiet(world: &World) -> bool {
    dirty_entity_count(world) == 0
}
"#;
    let source = r#"
fn idle(c: &mut Criterion) {
    let ran = guard::Ran::new();
    group.bench_function("x", |b| {
        b.iter(|| {
            ran.bump();
            run_fixed_tick(world);
        });
    });
    check_distinguishable(world, actors);
    let again = check_distinguishable(world, actors);
    let settled = quiet(world);
}
"#;
    let found = audit_with(source, &[("shape_checks".to_string(), helpers.to_string())]);
    assert_eq!(
        found.violations.len(),
        2,
        "a post-loop call to a helper that asserts inside must read as a violation whatever it is \
         named and whether or not a `let` catches its result, while a helper that only hands back a \
         bool stays clean; found {:#?}",
        found.violations,
    );

    let guarded = source.replace(
        "    check_distinguishable(world, actors);\n    let again = check_distinguishable(world, actors);",
        "    ran.guard(\"floor/idle_tick/x\", || {\n        check_distinguishable(world, actors);\n        let again = check_distinguishable(world, actors);\n    });",
    );
    assert_eq!(
        audit_with(
            &guarded,
            &[("shape_checks".to_string(), helpers.to_string())]
        )
        .violations,
        Vec::<String>::new(),
        "both calls behind one `.guard(`, and `quiet` never was a check the rule needed to see",
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

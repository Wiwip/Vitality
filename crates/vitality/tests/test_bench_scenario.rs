//! Guards for the bench harness the gate never builds: the share each `scenario/*` workload moves,
//! the three sums of the `expr` ladder, and the `#[path]` wiring that keeps the support modules live.

#[path = "../benches/support/common.rs"]
mod common;
#[path = "../benches/support/expr.rs"]
mod expr;
#[path = "../benches/support/scenario.rs"]
mod scenario;

use bevy::prelude::*;
use common::ATTRIBUTE_POOL;
use expr::{NODE_COUNTS, eval_erased, eval_mono, eval_typemap, expected, expr_fixture};
use scenario::{Workload, build, build_in};
use vitality::effect::ApplyEffectEvent;

const ACTORS: usize = 40;

fn bases(world: &mut scenario::ScenarioWorld, attr: usize) -> Vec<f64> {
    let actors = world.actors.clone();
    actors
        .iter()
        .map(|&actor| ATTRIBUTE_POOL[attr].base(world.app.world(), actor).unwrap())
        .collect()
}

fn moved(before: &[f64], after: &[f64]) -> usize {
    before.iter().zip(after).filter(|(a, b)| a != b).count()
}

// README's Performance numbers claim exactly these shares: steady moves 10 % of the actors, storm all
#[test]
fn write_workloads_touch_the_share_their_id_names() {
    for (workload, share) in [
        (Workload::Idle, 0),
        (Workload::Steady, ACTORS / 10),
        (Workload::Storm, ACTORS),
        (Workload::NoOpWrite, 0),
        (Workload::Damage, ACTORS / 10),
    ] {
        let mut world = build(workload, ACTORS);
        let before = bases(&mut world, 0);
        world.drive();
        world.tick();
        let after = bases(&mut world, 0);
        assert_eq!(moved(&before, &after), share, "{workload:?}");
    }
}

/// `World::trigger` runs its observers inline, so a `drive` that triggered instead of queueing has
/// already paid for the tag test, the condition eval and the stacking decision before the sample starts.
/// Nothing else builds this harness: an apply workload that applies nothing would still run a green tick.
#[test]
fn drive_queues_the_applies_and_tick_dispatches_them() {
    for (workload, applies) in [
        (Workload::BuffChurn, ACTORS / 10),
        (Workload::Combat, ACTORS / 5),
        (Workload::StackAdd, ACTORS / 10),
        (Workload::StackRefresh, ACTORS / 10),
        (Workload::TagRequired, ACTORS / 5),
        (Workload::Immunity, ACTORS / 5),
        (Workload::Snapshot, ACTORS / 10),
    ] {
        let mut world = build_in(workload, ACTORS);
        // on the world: `App::add_observer` waits for the next `First`, which this tick never runs
        world
            .app
            .world_mut()
            .insert_resource(Dispatched(Vec::new()));
        world.app.world_mut().add_observer(
            |ev: On<ApplyEffectEvent>, mut dispatched: ResMut<Dispatched>| {
                dispatched.0.push(ev.entity);
            },
        );
        world.drive();
        let queued = world.app.world().resource::<Dispatched>().0.len();
        assert_eq!(queued, 0, "{workload:?}: drive must not dispatch an apply");
        world.tick();
        let dispatched = &world.app.world().resource::<Dispatched>().0;
        assert_eq!(
            dispatched.len(),
            applies,
            "{workload:?}: the timed window must contain every apply drive queued"
        );
    }
}

#[derive(Resource, Default)]
struct Dispatched(Vec<Entity>);

// the typemap arm is bench-only, so nothing else catches the group timing three different sums
#[test]
fn expr_values_match() {
    let fixture = expr_fixture();
    for (nodes, erased) in &fixture.erased {
        let want = expected(&fixture, *nodes);
        assert!(want != 0.0, "a zero sum would hide a missed read");
        assert_eq!(
            eval_erased(&fixture, erased) as f64,
            want,
            "erased, {nodes} nodes"
        );
        assert_eq!(
            eval_mono(&fixture, *nodes) as f64,
            want,
            "mono, {nodes} nodes"
        );
        assert_eq!(
            eval_typemap(&fixture, *nodes),
            want,
            "typemap, {nodes} nodes"
        );
    }
    assert_eq!(fixture.erased.len(), NODE_COUNTS.len());
}

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

const BENCH_TARGET: &str = "bench_systems";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(file: &Path) -> String {
    fs::read_to_string(file).unwrap_or_else(|err| {
        panic!("bench wiring guard cannot read {}: {err}", file.display());
    })
}

fn list_rs(dir: &Path) -> Vec<PathBuf> {
    let entries = fs::read_dir(dir).unwrap_or_else(|err| {
        panic!(
            "bench wiring guard cannot read dir {}: {err}",
            dir.display()
        );
    });
    let mut files: Vec<PathBuf> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|err| {
                    panic!("bench wiring guard cannot read {}: {err}", dir.display())
                })
                .path()
        })
        .filter(|file| file.is_file() && file.extension() == Some(OsStr::new("rs")))
        .collect();
    files.sort();
    files
}

/// collapses `..` and `.` so a test's `../benches/support/x.rs` and the bench's `support/x.rs` match
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn path_attrs(file: &Path) -> Vec<PathBuf> {
    let dir = file
        .parent()
        .unwrap_or_else(|| panic!("{} has no parent directory", file.display()));
    read(file)
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("#[path")?;
            let quoted = rest.split_once('"')?.1.split('"').next()?;
            Some(normalize(&dir.join(quoted)))
        })
        .collect()
}

fn bench_file() -> PathBuf {
    manifest_dir()
        .join("benches")
        .join(format!("{BENCH_TARGET}.rs"))
}

fn support_files() -> Vec<PathBuf> {
    list_rs(&manifest_dir().join("benches/support"))
}

fn test_files() -> Vec<PathBuf> {
    let mut files = list_rs(&manifest_dir().join("tests"));
    files.retain(|file| {
        file.file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("test_"))
    });
    files
}

fn includer(module: &Path) -> Option<PathBuf> {
    test_files()
        .into_iter()
        .find(|test| path_attrs(test).iter().any(|included| included == module))
}

// the gate builds tests, never bench targets: a support module no test includes compiles nowhere
#[test]
fn every_support_module_is_declared_and_included() {
    let declared: BTreeSet<PathBuf> = path_attrs(&bench_file()).into_iter().collect();
    let modules = support_files();
    let scanned: Vec<String> = test_files()
        .iter()
        .map(|file| file.file_name().unwrap().to_string_lossy().into_owned())
        .collect();

    let undeclared: Vec<String> = modules
        .iter()
        .filter(|module| !declared.contains(&normalize(module)))
        .map(|module| module.display().to_string())
        .collect();
    assert!(
        undeclared.is_empty(),
        "rule 2 (support modules must be wired): the bench target {} declares no path module for \
         {undeclared:?}",
        bench_file().display()
    );

    let untested: Vec<String> = modules
        .iter()
        .filter(|module| includer(module).is_none())
        .map(|module| module.display().to_string())
        .collect();
    assert!(
        untested.is_empty(),
        "rule 2 (support modules must be wired): no test includes {untested:?} with a path \
         attribute (scanned {scanned:?}); the gate compiles tests and never bench targets, so a \
         support module nothing includes is code that never compiles until `cargo bench --no-run` \
         says so",
    );
}

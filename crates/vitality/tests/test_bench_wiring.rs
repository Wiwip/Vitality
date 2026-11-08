//! Guard for the bench wiring the gate cannot see: `scripts/gate.sh` runs
//! `cargo test --workspace --lib --tests`, which never builds a bench target, so a second top-level
//! `benches/*.rs`, a `support/` module nothing includes, and criterion in a support module all stay
//! green until a cold `cargo bench --no-run` says so eight minutes later.
//!
//! Paths come from `CARGO_MANIFEST_DIR`, never cwd, because `file!()` already differs between the
//! test and the bench build (D-19).

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

/// `.rs` files directly in `dir`, sorted, so a report names them in a stable order.
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

/// Collapses `..` and `.` so the bench target's `support/common.rs` and a test's
/// `../benches/support/common.rs` land on one path.
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

/// Every `#[path = "..."]` in a file, resolved against that file's own directory.
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

/// The `name` of every `[[bench]]` table, read line by line because no toml parser is in reach.
fn bench_names(cargo: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_bench = false;
    for line in cargo.lines() {
        let line = line.trim();
        if line.starts_with("[[bench]]") {
            in_bench = true;
            continue;
        }
        if line.starts_with('[') {
            in_bench = false;
            continue;
        }
        if !in_bench || !line.starts_with("name") {
            continue;
        }
        if let Some(quoted) = line.split_once('=')
            && let Some(name) = quoted
                .1
                .split_once('"')
                .and_then(|(_, rest)| rest.split('"').next())
        {
            names.push(name.to_string());
        }
    }
    names
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

/// The first `tests/test_*.rs` that includes `module` with a path attribute.
fn includer(module: &Path) -> Option<PathBuf> {
    test_files()
        .into_iter()
        .find(|test| path_attrs(test).iter().any(|included| included == module))
}

/// Rule 1: exactly one bench target, exactly one top-level bench file, and they are the same one.
#[test]
fn one_bench_target_for_one_bench_file() {
    let manifest = manifest_dir();
    assert!(
        bench_file().is_file(),
        "rule 1 (one bench target): {} is missing, but Cargo.toml declares the bench target",
        bench_file().display()
    );

    let stray: Vec<String> = list_rs(&manifest.join("benches"))
        .into_iter()
        .filter(|file| file.file_stem() != Some(OsStr::new(BENCH_TARGET)))
        .map(|file| file.display().to_string())
        .collect();
    assert!(
        stray.is_empty(),
        "rule 1 (one bench target): stray top-level bench file(s) {stray:?} beside {}\n\
         cargo turns any second benches/*.rs into a harness = true target that cannot link, and the \
         gate never builds it; shared code belongs in benches/support/ and a group belongs in the \
         bench target's own resolver_benches.",
        bench_file().display()
    );

    let cargo_path = manifest.join("Cargo.toml");
    let declared = bench_names(&read(&cargo_path));
    assert_eq!(
        declared,
        [BENCH_TARGET.to_string()],
        "rule 1 (one bench target): {} must declare exactly one bench target named \
         \"{BENCH_TARGET}\", found {declared:?}",
        cargo_path.display()
    );
}

/// Rule 2: every support module on disk is declared by the bench target and included by a test, so
/// the gate really compiles it.
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

/// Rule 3: criterion stays out of `benches/support/`, which is the only reason rule 2 can hold.
#[test]
fn support_modules_stay_criterion_free() {
    let mut offenders: Vec<String> = Vec::new();
    for module in support_files() {
        let source = read(&module);
        for (number, line) in source.lines().enumerate() {
            let code = line
                .trim()
                .trim_start_matches("pub ")
                .trim_start_matches("pub(crate) ");
            if code.starts_with("use criterion") {
                offenders.push(format!(
                    "{}:{}: {}",
                    module.display(),
                    number + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "rule 3 (no criterion in benches/support/): {offenders:?}\n\
         every support module documents that it stays criterion-free so tests can include it; a \
         criterion import belongs in the bench target {} alone.",
        bench_file().display()
    );
}

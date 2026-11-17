//! Guard for the bench wiring the gate cannot see: `cargo test --lib --tests` never builds a bench
//! target. Paths come from `CARGO_MANIFEST_DIR`, never cwd, because `file!()` differs between the
//! test and the bench build.

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

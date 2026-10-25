//! `--auto-gen-config`, `--regenerate-todo`, `--report-unused-todo-entries` and
//! `--disable-uncorrectable`: rbcop run on a copy of tests/auto_gen/<case>/input must leave the
//! tree RuboCop left (expected/, every file byte for byte) and print what it printed (recorded by
//! tests/auto_gen/record.sh).

mod common;

use std::fs;
use std::path::{Path, PathBuf};

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            fs::copy(e.path(), target).unwrap();
        }
    }
}

/// Every file under `dir`, relative to it, sorted.
fn files(dir: &Path) -> Vec<PathBuf> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            if e.path().is_dir() {
                walk(base, &e.path(), out);
            } else {
                out.push(e.path().strip_prefix(base).unwrap().to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn auto_gen_matches_rubocop() {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/auto_gen"));
    let mut cases: Vec<PathBuf> =
        fs::read_dir(root).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    cases.sort();
    let mut failures = Vec::new();
    for dir in cases {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let input = match fs::read_to_string(dir.join("input_from")) {
            Ok(from) => root.join(from.trim()).join("input"),
            Err(_) => dir.join("input"),
        };
        let work = std::env::temp_dir().join(format!("rbcop-auto-gen-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&work);
        copy_dir(&input, &work);
        let args = fs::read_to_string(dir.join("args")).unwrap();
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .current_dir(&work)
            .env("PWD", &work)
            .args(["--cache", "false"])
            .args(args.split_whitespace())
            .output()
            .unwrap();
        let read = |p: &str| fs::read_to_string(dir.join(p)).unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr: String = String::from_utf8_lossy(&out.stderr)
            .split_inclusive('\n')
            .filter(|l| !l.starts_with("rbcop: not running "))
            .collect();
        if stdout != read("stdout") {
            failures.push(format!("{name}: stdout\n--- expected\n{}--- actual\n{stdout}", read("stdout")));
        }
        if stderr != read("stderr") {
            failures.push(format!("{name}: stderr\n--- expected\n{}--- actual\n{stderr}", read("stderr")));
        }
        let status = out.status.code().unwrap_or(-1).to_string();
        if status != read("status").trim() {
            failures.push(format!("{name}: exit {status}, expected {}", read("status").trim()));
        }
        let expected = dir.join("expected");
        let (want, got) = (files(&expected), files(&work));
        if want != got {
            failures.push(format!("{name}: files {got:?}, expected {want:?}"));
        }
        for f in want.iter().filter(|f| got.contains(f)) {
            let (a, b) = (fs::read(expected.join(f)).unwrap(), fs::read(work.join(f)).unwrap());
            if a != b {
                failures.push(format!(
                    "{name}: {}\n--- expected\n{}--- actual\n{}",
                    f.display(),
                    String::from_utf8_lossy(&a),
                    String::from_utf8_lossy(&b)
                ));
            }
        }
        let _ = fs::remove_dir_all(&work);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

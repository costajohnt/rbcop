//! `rbcop -A` on tests/autocorrect/<case>/input must rewrite files and report offenses exactly as
//! `rubocop -A` did (recorded by tests/autocorrect/record.sh into expected/ and expected.txt).

mod common;

use std::fs;
use std::path::Path;

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        fs::copy(e.path(), to.join(e.file_name())).unwrap();
    }
}

#[test]
fn autocorrect_matches_rubocop() {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/autocorrect"));
    // Every case directory (each has input/, expected/, expected.txt and config.yml).
    let mut cases: Vec<String> = fs::read_dir(root)
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    cases.sort();
    for case in &cases {
        let dir = root.join(case);
        // A case may list its own cops (`<case>/cops.txt`).
        let cops =
            fs::read_to_string(dir.join("cops.txt")).or_else(|_| fs::read_to_string(root.join("cops.txt"))).unwrap();
        let work = std::env::temp_dir().join(format!("rbcop-autocorrect-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&work);
        copy_dir(&dir.join("input"), &work);
        // `all`: no --only (as tests/autocorrect/record.sh); config.yml picks the cops.
        let only = if cops.trim() == "all" { vec![] } else { vec!["--only", cops.trim()] };
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .current_dir(&work)
            .args(["-A", "--format", "emacs"])
            .args(only)
            .arg("-c")
            .arg(dir.join("config.yml"))
            .arg(".")
            .output()
            .unwrap();
        let prefix = format!("{}/", work.canonicalize().unwrap().display());
        let report = String::from_utf8(out.stdout).unwrap().replace(&prefix, "");
        assert_eq!(report, fs::read_to_string(dir.join("expected.txt")).unwrap(), "{case}: report");
        for e in fs::read_dir(dir.join("expected")).unwrap().flatten() {
            // Bytes: a file in another encoding stays as it was when RuboCop rejects it.
            let actual = fs::read(work.join(e.file_name())).unwrap();
            assert_eq!(actual, fs::read(e.path()).unwrap(), "{case}: {:?}", e.file_name());
        }
        // 1 (uncorrected offenses remain) unless the case records RuboCop's status (`<case>/status`).
        let status = fs::read_to_string(dir.join("status")).map_or(1, |s| s.trim().parse().unwrap());
        assert_eq!(out.status.code(), Some(status), "{case}: status");
        fs::remove_dir_all(&work).unwrap();
    }
}

/// `rbcop --diff -A` prints what `rubocop --diff -A` printed (diff.txt, recorded by record.sh for
/// the cases without a deliberate difference) and leaves input/ as it was (#290).
#[test]
fn diff_matches_rubocop() {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/autocorrect"));
    let mut cases: Vec<_> = fs::read_dir(root).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    cases.sort();
    let mut count = 0;
    for dir in cases.iter().filter(|d| d.join("diff.txt").is_file()) {
        let case = dir.file_name().unwrap().to_string_lossy();
        let cops =
            fs::read_to_string(dir.join("cops.txt")).or_else(|_| fs::read_to_string(root.join("cops.txt"))).unwrap();
        let work = std::env::temp_dir().join(format!("rbcop-diff-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&work);
        copy_dir(&dir.join("input"), &work);
        let only = if cops.trim() == "all" { vec![] } else { vec!["--only", cops.trim()] };
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .current_dir(&work)
            .args(["--diff", "-A", "--format", "emacs"])
            .args(only)
            .arg("-c")
            .arg(dir.join("config.yml"))
            .arg(".")
            .output()
            .unwrap();
        let prefix = format!("{}/", work.canonicalize().unwrap().display());
        let report = String::from_utf8(out.stdout).unwrap().replace(&prefix, "");
        assert_eq!(report, fs::read_to_string(dir.join("diff.txt")).unwrap(), "{case}: --diff output");
        for e in fs::read_dir(dir.join("input")).unwrap().flatten() {
            assert_eq!(fs::read(work.join(e.file_name())).unwrap(), fs::read(e.path()).unwrap(), "{case}: written");
        }
        assert_eq!(out.status.code(), Some(1), "{case}: status");
        fs::remove_dir_all(&work).unwrap();
        count += 1;
    }
    assert!(count > 20, "only {count} cases have diff.txt");
}

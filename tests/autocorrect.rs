//! `rbcop -A` on tests/autocorrect/<case>/input must rewrite files and report offenses exactly as
//! `rubocop -A` did (recorded by tests/autocorrect/record.sh into expected/ and expected.txt).

use std::fs;
use std::path::Path;
use std::process::Command;

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        fs::copy(e.path(), to.join(e.file_name())).unwrap();
    }
}

#[test]
fn autocorrect_matches_rubocop() {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/autocorrect"));
    let cops = fs::read_to_string(root.join("cops.txt")).unwrap();
    for case in ["comparison", "default"] {
        let dir = root.join(case);
        let work = std::env::temp_dir().join(format!("rbcop-autocorrect-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&work);
        copy_dir(&dir.join("input"), &work);
        let out = Command::new(env!("CARGO_BIN_EXE_rbcop"))
            .current_dir(&work)
            .args(["-A", "--format", "emacs", "--only", cops.trim(), "-c"])
            .arg(dir.join("config.yml"))
            .arg(".")
            .output()
            .unwrap();
        let prefix = format!("{}/", work.canonicalize().unwrap().display());
        let report = String::from_utf8(out.stdout).unwrap().replace(&prefix, "");
        assert_eq!(report, fs::read_to_string(dir.join("expected.txt")).unwrap(), "{case}: report");
        for e in fs::read_dir(dir.join("expected")).unwrap().flatten() {
            let actual = fs::read_to_string(work.join(e.file_name())).unwrap();
            assert_eq!(actual, fs::read_to_string(e.path()).unwrap(), "{case}: {:?}", e.file_name());
        }
        assert_eq!(out.status.code(), Some(1), "{case}: uncorrected offenses remain");
        fs::remove_dir_all(&work).unwrap();
    }
}

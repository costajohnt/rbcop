//! Formatter and CLI goldens: rbcop's stdout and exit status must equal RuboCop's, recorded by
//! scripts/formatters.sh from tests/formatters/cases.txt into tests/formatters/expected/.

mod common;

use std::path::Path;
use std::process::Stdio;

/// Same substitutions as normalise() in scripts/formatters.sh.
fn normalise(s: &str, root: &str) -> String {
    let platform = regex::Regex::new(r#""ruby_platform":"[^"]*""#).unwrap();
    let time = regex::Regex::new(r"Finished in [0-9.e-]* seconds").unwrap();
    let verbose = regex::Regex::new(r"(?m)\) \[[^\]]*\]$").unwrap();
    let s = s.replace(root, "<root>");
    let s = time.replace(&s, "Finished in <time> seconds");
    let s = platform.replace(&s, r#""ruby_platform":"<platform>""#);
    verbose.replace(&s, ") [<platform>]").into_owned()
}

#[test]
fn formatters_match_rubocop() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap();
    let dir = root.join("tests/formatters");
    let cops = std::fs::read_to_string(dir.join("cops.txt")).unwrap().lines().collect::<Vec<_>>().join(",");
    let root_s = root.display().to_string();
    let mut failures = Vec::new();
    let mut count = 0;
    for line in std::fs::read_to_string(dir.join("cases.txt")).unwrap().lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(4, '|');
        let (name, cwd_name, args) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        let stdin = match parts.next() {
            Some(f) => Stdio::from(std::fs::File::open(root.join(f)).unwrap()),
            None => Stdio::null(),
        };
        let mut cwd = root.join(cwd_name);
        // fix-* cases autocorrect, so they run on a fresh copy of their directory.
        if name.starts_with("fix-") {
            let work = std::env::temp_dir().join(format!("rbcop-formatters-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&work);
            std::fs::create_dir_all(&work).unwrap();
            for e in std::fs::read_dir(&cwd).unwrap().flatten() {
                std::fs::copy(e.path(), work.join(e.file_name())).unwrap();
            }
            cwd = work.canonicalize().unwrap();
        }
        // `sh -c` so the arguments split and quote exactly as in the recording script.
        let out = common::command("sh")
            .arg("-c")
            .arg(format!("\"$RBCOP\" --cache false --force-default-config --only {cops} {args}"))
            .env("RBCOP", env!("CARGO_BIN_EXE_rbcop"))
            .env("RUBY_PROGRESS_BAR_LENGTH", "80")
            .env("PWD", &cwd)
            .current_dir(&cwd)
            .stdin(stdin)
            .output()
            .unwrap();
        count += 1;
        if name.starts_with("fix-") {
            // `--diff` writes nothing.
            if name.starts_with("fix-diff-") {
                for e in std::fs::read_dir(root.join(cwd_name)).unwrap().flatten() {
                    let copy = std::fs::read(cwd.join(e.file_name())).unwrap();
                    assert!(copy == std::fs::read(e.path()).unwrap(), "{name}: {:?} was written", e.file_name());
                }
            }
            let _ = std::fs::remove_dir_all(&cwd);
        }
        let expected = std::fs::read_to_string(dir.join(format!("expected/{name}.txt"))).unwrap();
        let status: i32 =
            std::fs::read_to_string(dir.join(format!("expected/{name}.status"))).unwrap().trim().parse().unwrap();
        let actual = normalise(&String::from_utf8_lossy(&out.stdout), &root_s);
        if actual != expected || out.status.code() != Some(status) {
            let diff: Vec<String> = expected
                .lines()
                .zip(actual.lines().chain(std::iter::repeat("<missing>")))
                .enumerate()
                .filter(|(_, (e, a))| e != a)
                .take(3)
                .map(|(i, (e, a))| format!("    line {}:\n      rubocop: {e:?}\n      rbcop:   {a:?}", i + 1))
                .collect();
            failures.push(format!(
                "{name}: exit {:?} (rubocop {status}), {} vs {} lines\n{}\n    stderr: {}",
                out.status.code(),
                actual.lines().count(),
                expected.lines().count(),
                diff.join("\n"),
                String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("")
            ));
        }
    }
    assert!(count > 50, "only {count} cases");
    assert!(failures.is_empty(), "{} of {count} cases differ:\n{}", failures.len(), failures.join("\n"));
}

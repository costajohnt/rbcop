//! `--show-cops` must print exactly what RuboCop prints (tests/show_cops/expected, recorded by
//! scripts/show-cops.sh in tests/show_cops/project).

use std::path::Path;
use std::process::Command;

/// Keep in sync with scripts/show-cops.sh.
const CASES: &[(&str, &[&str])] = &[
    ("all", &["--show-cops"]),
    (
        "patterns",
        &["--show-cops", "Lint/*,Style/StringLiterals,Style/InverseMethods,Metrics/MethodLength,Naming/MethodName"],
    ),
    ("preview", &["--preview", "--show-cops", "Layout/*,Bundler/OrderedGems,Metrics/BlockLength"]),
    (
        "default",
        &["--force-default-config", "--show-cops", "Layout/*,Lint/MissingCopEnableDirective,Lint/UriEscapeUnescape"],
    ),
];

#[test]
fn show_cops_matches_rubocop() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap().join("tests/show_cops");
    let project = dir.join("project");
    let mut failures = Vec::new();
    for (name, args) in CASES {
        let out = Command::new(env!("CARGO_BIN_EXE_rbcop"))
            .args(*args)
            .current_dir(&project)
            .env("PWD", &project)
            .env_remove("RUBOCOP_OPTS")
            .output()
            .unwrap();
        let actual = String::from_utf8(out.stdout).unwrap().replace(&project.display().to_string(), "<project>");
        let expected = std::fs::read_to_string(dir.join(format!("expected/{name}.txt"))).unwrap();
        if actual != expected {
            let first = expected
                .lines()
                .zip(actual.lines().chain(std::iter::repeat("<missing>")))
                .enumerate()
                .find(|(_, (e, a))| e != a)
                .map(|(i, (e, a))| format!("line {}: rubocop {e:?}, rbcop {a:?}", i + 1))
                .unwrap_or_else(|| "rbcop prints extra lines".into());
            failures.push(format!("{name}: {first}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

//! Config scenarios: rbcop must reproduce RuboCop's recorded output (tests/scenarios/*/expected.txt,
//! written by scripts/scenarios.sh) for every cop rbcop implements.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_rbcop")
}

fn implemented() -> BTreeSet<String> {
    let out = Command::new(bin()).arg("--show-cops").output().unwrap();
    // `--show-cops` prints RuboCop's YAML; cop names are the unindented keys.
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with([' ', '#']))
        .filter_map(|l| l.strip_suffix(':'))
        .map(String::from)
        .collect()
}

/// Cop name from an emacs-format line: `path:l:c: C: [Correctable] Cop/Name: message`.
fn cop_of(line: &str) -> Option<&str> {
    // Skip "path:l:c" and the severity letter.
    let rest = &line[line.find(": ")? + 2..];
    let rest = &rest[rest.find(": ")? + 2..];
    let rest = rest.strip_prefix("[Correctable] ").unwrap_or(rest);
    rest.split(':').next()
}

#[test]
fn config_scenarios_match_rubocop() {
    let cops = implemented();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios");
    let mut failures = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    for dir in names {
        let project = dir.join("project");
        let args = std::fs::read_to_string(dir.join("args")).unwrap_or_default();
        let out = Command::new(bin())
            .args(["--format", "emacs"])
            .args(args.split_whitespace())
            .current_dir(&project)
            .env("PWD", &project)
            .output()
            .unwrap();
        let prefix = format!("{}/", project.display());
        let mut actual: Vec<String> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|l| l.strip_prefix(&prefix).unwrap_or(l).to_string())
            .collect();
        actual.sort();
        let mut expected: Vec<String> = std::fs::read_to_string(dir.join("expected.txt"))
            .unwrap()
            .lines()
            // Lint/Syntax is not in --show-cops output but rbcop always reports it.
            .filter(|l| cop_of(l).is_some_and(|c| c == "Lint/Syntax" || cops.contains(c)))
            .map(String::from)
            .collect();
        expected.sort();
        if actual != expected {
            let missing: Vec<_> = expected.iter().filter(|l| !actual.contains(l)).collect();
            let extra: Vec<_> = actual.iter().filter(|l| !expected.contains(l)).collect();
            failures.push(format!(
                "{}:\n  only in rubocop:\n    {}\n  only in rbcop:\n    {}\n  stderr: {}",
                dir.file_name().unwrap().to_string_lossy(),
                missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n    "),
                extra.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n    "),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

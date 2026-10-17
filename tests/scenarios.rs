//! Config scenarios: rbcop must reproduce RuboCop's recorded output (tests/scenarios/*/expected.txt,
//! written by scripts/scenarios.sh) for every cop rbcop implements.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_rbcop")
}

fn implemented() -> BTreeSet<String> {
    // With each native plugin loaded, so their cops are listed too.
    let mut cops = BTreeSet::new();
    for plugin in ["performance", "rails", "minitest", "rspec"] {
        let config = format!("{}/config/all-cops-{plugin}.yml", env!("CARGO_MANIFEST_DIR"));
        let out = common::command(bin()).args(["--show-cops", "--config", &config]).output().unwrap();
        // `--show-cops` prints RuboCop's YAML; cop names are the unindented keys.
        cops.extend(
            String::from_utf8(out.stdout)
                .unwrap()
                .lines()
                .filter(|l| !l.starts_with([' ', '#']))
                .filter_map(|l| l.strip_suffix(':'))
                .map(String::from),
        );
    }
    cops
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
        let out = common::command(bin())
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
        // stderr.txt, when recorded, is compared byte for byte (it covers every RuboCop cop).
        // Absolute paths in it are recorded relative to the project, like stdout.
        // The skipped-cops notice (src/fallback.rs) is a deliberate difference, tested in tests/fallback.rs.
        let stderr: String = String::from_utf8_lossy(&out.stderr)
            .replace(&prefix, "")
            .split_inclusive('\n')
            .filter(|l| !l.starts_with("rbcop: not running "))
            .collect();
        if let Ok(err) = std::fs::read_to_string(dir.join("stderr.txt"))
            && err != stderr
        {
            failures.push(format!(
                "{}: stderr differs\n--- rubocop\n{err}\n--- rbcop\n{}",
                dir.file_name().unwrap().to_string_lossy(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        // status, when recorded, is RuboCop's exit status.
        if let Ok(status) = std::fs::read_to_string(dir.join("status"))
            && status.trim().parse::<i32>().ok() != out.status.code()
        {
            failures.push(format!(
                "{}: exit status {:?}, rubocop {}",
                dir.file_name().unwrap().to_string_lossy(),
                out.status.code(),
                status.trim()
            ));
        }
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

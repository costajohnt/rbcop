//! `--show-cops` must print exactly what RuboCop prints (tests/show_cops/expected, recorded by
//! scripts/show-cops.sh in tests/show_cops/project).

mod common;

use std::path::Path;

/// Keep in sync with scripts/show-cops.sh. (name, project dir, args); `plugins` loads rubocop-rspec
/// and rubocop-capybara (the nested department Capybara/RSpec).
const CASES: &[(&str, &str, &[&str])] = &[
    ("all", "project", &["--show-cops"]),
    (
        "patterns",
        "project",
        &["--show-cops", "Lint/*,Style/StringLiterals,Style/InverseMethods,Metrics/MethodLength,Naming/MethodName"],
    ),
    ("preview", "project", &["--preview", "--show-cops", "Layout/*,Bundler/OrderedGems,Metrics/BlockLength"]),
    (
        "default",
        "project",
        &["--force-default-config", "--show-cops", "Layout/*,Lint/MissingCopEnableDirective,Lint/UriEscapeUnescape"],
    ),
    ("plugins_all", "plugins", &["--show-cops"]),
    ("plugins_list_enabled", "plugins", &["--list-enabled-cops-for", "spec/a_spec.rb"]),
    (
        "plugins_docs_url",
        "plugins",
        &["--show-docs-url", "Capybara/RSpec/HaveContent,Capybara/AssertStyle,Capybara/RSpec,RSpec/Focus,Style/Not"],
    ),
    // junit lists a file's testcases in registry order: plugins in load order (rails first here).
    ("junit_order", "junit_order", &["-f", "junit", "a.rb"]),
];

#[test]
fn show_cops_matches_rubocop() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap().join("tests/show_cops");
    let mut failures = Vec::new();
    for (name, project, args) in CASES {
        let project = dir.join(project);
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(*args)
            .current_dir(&project)
            .env("PWD", &project)
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

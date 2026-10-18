//! Cop error locations (Commissioner#with_cop_error_handling: the node the callback was given):
//! a block is the parser gem's block node, starting at its call, and Metrics/ParameterLists counts
//! parameters in `on_args` (#596). The project is tests/scenarios/cop_error_location_block_and_args,
//! whose `# rubocop:disable Output` (ambiguous between Rails/Output and RSpec/Output) makes every
//! offense a cop error. RuboCop 1.91.0's lines, sorted (its order differs; see the scenario).

mod common;

#[test]
fn cop_errors_are_located_at_the_parser_node() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/scenarios/cop_error_location_block_and_args/project");
    let out =
        common::command(env!("CARGO_BIN_EXE_rbcop")).args(["-f", "emacs", "a.rb"]).current_dir(&dir).output().unwrap();
    let prefix = format!("{}/", dir.display());
    let stderr = String::from_utf8_lossy(&out.stderr).replace(&prefix, "");
    let mut errors: Vec<&str> = stderr.lines().filter(|l| l.starts_with("An error") && l.contains("a.rb:")).collect();
    errors.sort();
    let at = |cop: &str, loc: &str| format!("An error occurred while {cop} cop was inspecting a.rb:{loc}.");
    let expected = [
        at("Layout/SpaceAroundBlockParameters", "4:4"),
        at("Layout/SpaceAroundBlockParameters", "5:0"),
        at("Metrics/ParameterLists", "6:5"),
        at("Metrics/ParameterLists", "7:7"),
        at("Naming/MethodParameterName", "6:0"),
        at("Style/Lambda", "4:4"),
    ];
    assert_eq!(errors, expected, "{stderr}");
}

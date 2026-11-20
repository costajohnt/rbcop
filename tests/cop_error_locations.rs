//! Cop error locations (Commissioner#with_cop_error_handling: the node the callback was given):
//! a block is the parser gem's block node, starting at its call, and Metrics/ParameterLists counts
//! parameters in `on_args` (#596). The project is tests/scenarios/cop_error_location_block_and_args,
//! whose `# rubocop:disable Output` (ambiguous between Rails/Output and RSpec/Output) makes every
//! offense a cop error. RuboCop 1.92.1's lines, sorted (its order differs; see the scenario).
//! Layout/IndentationConsistency reads the comment config since 1.92 (governed_base_column).

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
        at("Layout/IndentationConsistency", "4:0"),
        at("Layout/SpaceAroundBlockParameters", "4:4"),
        at("Layout/SpaceAroundBlockParameters", "5:0"),
        at("Metrics/ParameterLists", "6:5"),
        at("Metrics/ParameterLists", "7:7"),
        at("Naming/MethodParameterName", "6:0"),
        at("Style/Lambda", "4:4"),
    ];
    assert_eq!(errors, expected, "{stderr}");
}

/// A String `Layout/LineLength` `Max` raises at every breakable node that compares it, even on a
/// short line, and in on_file (#779: the no-long-line shortcut must not skip them). RuboCop
/// 1.92.2's lines for tests/scenarios/line_length_string_max/project/short.rb.
#[test]
fn line_length_string_max_errors_at_short_lines() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios/line_length_string_max/project");
    let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--cache", "false", "-f", "emacs", "short.rb"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let prefix = format!("{}/", dir.display());
    let stderr = String::from_utf8_lossy(&out.stderr).replace(&prefix, "");
    let mut errors: Vec<&str> = stderr.lines().filter(|l| l.starts_with("An error")).collect();
    errors.sort();
    errors.dedup();
    let at = |loc: &str| format!("An error occurred while Layout/LineLength cop was inspecting short.rb{loc}.");
    assert_eq!(errors, [at(""), at(":3:0"), at(":4:4"), at(":7:7")], "{stderr}");
}

/// A `regexp =~ x` match whose regexp has the `s` or `e` option makes VariableForce raise
/// (rubocop-ast's RegexpNode#options has no bit for them): one cop error without a location per
/// file, and only the offenses of scopes left before it (#815). RuboCop 1.92.2's lines for
/// tests/scenarios/variable_force_regexp_encoding_option (offenses in its expected.txt).
#[test]
fn variable_force_errors_at_encoding_option_matches() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/scenarios/variable_force_regexp_encoding_option/project");
    let args = std::fs::read_to_string(dir.join("../args")).unwrap();
    let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--cache", "false", "-f", "emacs"])
        .args(args.split_whitespace())
        .current_dir(&dir)
        .output()
        .unwrap();
    let prefix = format!("{}/", dir.display());
    let stderr = String::from_utf8_lossy(&out.stderr).replace(&prefix, "");
    let mut errors: Vec<&str> = stderr.lines().filter(|l| l.starts_with("An error")).collect();
    errors.sort();
    let at = |file: &str| format!("An error occurred while VariableForce cop was inspecting {file}.");
    let mut expected: Vec<String> = ["e1.rb", "s1.rb", "s2.rb", "s3.rb", "s4.rb"].iter().flat_map(|f| [at(f), at(f)]).collect();
    expected.sort();
    assert_eq!(errors, expected, "{stderr}");
}

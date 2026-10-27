//! The server options (RuboCop::Server::CLI, run on ARGV before anything else). Output recorded
//! with RuboCop 1.92.1 while no server was running, which is always the case for rbcop (it runs in
//! process; docs/known-gaps.md).

mod common;

use std::fs;

#[test]
fn server_options_match_rubocop_without_a_running_server() {
    let dir = std::env::temp_dir().join(format!("rbcop-server-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  NewCops: enable\n  SuggestExtensions: false\n").unwrap();
    fs::write(dir.join("a.rb"), "x = 1\n").unwrap();
    const LINT: &str = "a.rb:1:1: W: [Correctable] Lint/UselessAssignment: Useless assignment to variable - `x`.\n\
                        a.rb:1:1: C: [Correctable] Style/FrozenStringLiteralComment: Missing frozen string literal comment.\n";
    const NOT_RUNNING: &str = "RuboCop server is not running.\n";
    // (RUBOCOP_OPTS, arguments, stdout, stderr, exit status)
    let cases: &[(&str, &[&str], &str, &str, i32)] = &[
        ("", &["--server-status"], NOT_RUNNING, "", 0),
        ("", &["--server-status", "a.rb"], NOT_RUNNING, "", 0),
        ("", &["--server-status", "--cache-root=x", "a.rb"], NOT_RUNNING, "", 0),
        ("", &["--stop-server"], "", NOT_RUNNING, 0),
        ("", &["--stop-server", "--stop-server"], "", NOT_RUNNING, 0),
        (
            "",
            &["--server-status", "--stop-server"],
            "",
            "--server-status, --stop-server cannot be specified together.\n",
            2,
        ),
        (
            "",
            &["--stop-server", "--server-status", "--no-server"],
            "",
            "--no-server, --server-status, --stop-server cannot be specified together.\n",
            2,
        ),
        (
            "",
            &["--no-detach", "--start-server", "--stop-server"],
            "",
            "--start-server, --stop-server cannot be specified together.\n",
            2,
        ),
        ("", &["--no-detach", "--server-status"], "", "--server-status cannot be combined with --no-detach.\n", 2),
        ("", &["--no-detach", "a.rb"], "", " cannot be combined with --no-detach.\n", 2),
        ("", &["--server-status", "-a", "a.rb"], "", "--server-status cannot be combined with -a.\n", 2),
        ("", &["--server-status", "--", "a.rb"], "", "--server-status cannot be combined with --.\n", 2),
        (
            "",
            &["--server-status", "--cache-root", "x", "a.rb"],
            "",
            "--server-status cannot be combined with --cache-root.\n",
            2,
        ),
        (
            "",
            &["--server-status", "--cache-root=x", "a.rb", "b"],
            "",
            "--server-status cannot be combined with --cache-root=x.\n",
            2,
        ),
        // `"--cache-root==".split('=')[1]` is nil: no cache root, so only one more argument is allowed.
        (
            "",
            &["--server-status", "--cache-root==", "a.rb"],
            "",
            "--server-status cannot be combined with --cache-root==.\n",
            2,
        ),
        ("", &["--no-server", "-f", "e", "a.rb"], LINT, "", 1),
        // OptionParser completes the abbreviation; Server::CLI only matches whole options.
        ("", &["--server-stat", "-f", "e", "a.rb"], LINT, "", 1),
        // Server::CLI reads only `--server` from RUBOCOP_OPTS; Options accepts the rest and lints.
        ("--server-status", &["-f", "e", "a.rb"], LINT, "", 1),
        ("--stop-server", &["-f", "e", "a.rb"], LINT, "", 1),
        ("--no-detach", &["-f", "e", "a.rb"], LINT, "", 1),
        // Deliberate differences: RuboCop starts a server here and prints
        // `RuboCop server starting on 127.0.0.1:<port>.` first; rbcop lints in process.
        ("", &["--server", "-f", "e", "a.rb"], LINT, "", 1),
        ("", &["--start-server"], "", "", 0),
        ("", &["--restart-server"], "", NOT_RUNNING, 0),
    ];
    let prefix = format!("{}/", dir.canonicalize().unwrap().display());
    for (opts, args, stdout, stderr, status) in cases {
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(*args)
            .env("RUBOCOP_OPTS", opts)
            .current_dir(&dir)
            .output()
            .unwrap();
        let got = String::from_utf8_lossy(&out.stdout).replace(&prefix, "");
        assert_eq!(
            (got.as_str(), String::from_utf8_lossy(&out.stderr).as_ref(), out.status.code()),
            (*stdout, *stderr, Some(*status)),
            "RUBOCOP_OPTS={opts:?} {args:?}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

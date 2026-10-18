//! Linting an untrusted file must not exhaust CPU or memory, overflow the stack, or fail a cop:
//! deep nesting, huge evaluated values and long names run every cop within a time budget.

mod common;

use std::fs;
use std::time::{Duration, Instant};

/// CPU time per run (every cop). Release builds take well under a second for each; CPU time,
/// not wall time, so a loaded machine (parallel gates) does not fail the test.
const BUDGET: Duration = Duration::from_secs(60);
/// Wall-clock backstop for a run that hangs without using CPU.
const WALL_BUDGET: Duration = Duration::from_secs(600);

/// RUSAGE_CHILDREN is per process: the tests' CPU measurements must not overlap.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// User + system CPU time of the waited-for child processes so far.
fn children_cpu() -> Duration {
    // SAFETY: getrusage only writes the zeroed struct it is given.
    let ru = unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_CHILDREN, &mut ru);
        ru
    };
    let tv = |t: libc::timeval| Duration::from_secs(t.tv_sec as u64) + Duration::from_micros(t.tv_usec as u64);
    tv(ru.ru_utime) + tv(ru.ru_stime)
}

fn inputs() -> Vec<(&'static str, String)> {
    let nest = |open: &str, close: &str, n: usize| format!("x = {}1{}\n", open.repeat(n), close.repeat(n));
    vec![
        // Nesting just under runner::MAX_AST_DEPTH runs every cop; deeper is skipped, not overflowed.
        ("at_limit_array.rb", nest("[", "]", 990)),
        ("at_limit_paren.rb", nest("(", ")", 490)),
        ("at_limit_if.rb", format!("{}b{}\n", "if a; ".repeat(990), "; end".repeat(990))),
        ("at_limit_ternary.rb", format!("x = {}b{}\n", "a ? ".repeat(990), " : c".repeat(990))),
        ("at_limit_def.rb", format!("{}b{}\n", "def m; ".repeat(490), "; end".repeat(490))),
        ("too_deep_array.rb", nest("[", "]", 5000)),
        ("too_deep_chain.rb", format!("x = a{}\n", ".b".repeat(100_000))),
        ("too_deep_plus.rb", format!("x = {}\n", vec!["a"; 100_000].join(" + "))),
        // Interpolated regexps are parsed by rbcop, not Prism.
        ("regexp_groups.rb", format!("x = /{}#{{a}}{}/\n", "(".repeat(100_000), ")".repeat(100_000))),
        ("regexp_sets.rb", format!("x = /{}#{{a}}{}/\n", "[a".repeat(100_000), "]".repeat(100_000))),
        // Values evaluated at lint time.
        (
            "format_width.rb",
            "format('%999999999d', 1)\nformat('%.999999999f', 1.0)\nformat('%*d', 999999999, 1)\n".into(),
        ),
        ("format_many.rb", format!("format('{}', {})\n", "%s".repeat(50_000), vec!["1"; 50_000].join(", "))),
        ("interpolated_hex.rb", format!("x = \"#{{0x{}}}\"\n", "f".repeat(200_000))),
        ("big_numbers.rb", "a = 10**999999999\nb = 1 << 999999999\nc = 'a' * 999999999\nd = 1.0e999999999\n".into()),
        // Names that end up in regexes or spell checking.
        ("long_block_arg.rb", format!("1.times {{ |{0}| puts {0} }}\n", "a".repeat(200_000))),
        ("long_cop_name.rb", format!("# rubocop:disable Style/{}\nx = 1\n", "A".repeat(200_000))),
        // Size-quadratic paths (sibling lookups, per-offense line copies).
        ("many_statements.rb", "x = 1\n".repeat(20_000)),
    ]
}

#[test]
fn hostile_inputs_stay_bounded() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("rbcop-hostile-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    let config = concat!(env!("CARGO_MANIFEST_DIR"), "/config/all-cops.yml");
    for (name, text) in inputs() {
        // `-A` re-indents deeply nested code over up to 200 iterations (docs/known-gaps.md), so the
        // nested-if/def/ternary inputs are only inspected.
        let modes: &[bool] = if matches!(name, "at_limit_if.rb" | "at_limit_ternary.rb" | "at_limit_def.rb") {
            &[false]
        } else {
            &[false, true]
        };
        for &autocorrect in modes {
            fs::write(dir.join(name), &text).unwrap();
            let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
            cmd.args(["--force-default-config", "-c", config, "--format", "emacs"]).current_dir(&dir);
            if autocorrect {
                cmd.arg("-A");
            }
            let (t0, cpu0) = (Instant::now(), children_cpu());
            let out = cmd.arg(name).output().unwrap();
            let (elapsed, cpu) = (t0.elapsed(), children_cpu() - cpu0);
            let stderr = String::from_utf8_lossy(&out.stderr);
            // 1 = offenses (or the too-deep error); a signal (stack overflow abort) has no code.
            assert!(matches!(out.status.code(), Some(0 | 1)), "{name} -A={autocorrect}: {:?}\n{stderr}", out.status);
            assert!(!stderr.contains("cop was inspecting"), "{name} -A={autocorrect}: cop error\n{stderr}");
            assert!(cpu < BUDGET, "{name} -A={autocorrect}: took {cpu:?} CPU");
            assert!(elapsed < WALL_BUDGET, "{name} -A={autocorrect}: took {elapsed:?}");
            assert_eq!(stderr.contains("levels deep, not inspected"), name.starts_with("too_deep"), "{name}\n{stderr}");
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Below 3.3 the parser-gem simulation walks the tree recursively: the depth guard runs before it,
/// so a long chain gets the too-deep error instead of a stack overflow (#351).
#[test]
fn too_deep_below_3_3_is_skipped() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("rbcop-hostile-legacy-deep-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  TargetRubyVersion: 3.2\n  NewCops: disable\n").unwrap();
    fs::write(dir.join("chain.rb"), format!("x = a{}\n", ".b".repeat(400_000))).unwrap();
    // A syntax error too: the simulation that reports it is the recursive part.
    fs::write(dir.join("chain_error.rb"), format!("x = a{} +\n", ".b".repeat(400_000))).unwrap();
    for name in ["chain.rb", "chain_error.rb"] {
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(["--only", "Style/StringLiterals", "--format", "emacs", name])
            .current_dir(&dir)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{name}: {:?}\n{stderr}", out.status);
        assert!(
            stderr.contains(&format!("rbcop: {name}: nested more than 1000 levels deep, not inspected.")),
            "{stderr}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// A backtracking config pattern's run budget is per file: a file's offenses are the same linted
/// alone or after other files that exhaust the pattern, whichever worker thread gets them (#324).
/// `z.rb` gives up 31 times (under the budget) and then has a cheap match; with a budget shared by
/// the run, the 32 files before it (at least one on its thread) used it up and the match was lost.
#[test]
fn config_regex_budget_is_per_file() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("rbcop-hostile-budget-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join(".rubocop.yml"), "Layout/LineLength:\n  Max: 10\n  AllowedPatterns: ['(a|aa)+(?=d)|B']\n")
        .unwrap();
    let hard = format!("# {}\n", "a".repeat(60));
    for i in 0..32 {
        fs::write(dir.join(format!("h{i:02}.rb")), &hard).unwrap();
    }
    fs::write(dir.join("z.rb"), format!("{}# B {}\n", hard.repeat(31), "x".repeat(20))).unwrap();
    let run = |files: &[&str]| {
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(["--only", "Layout/LineLength", "--format", "emacs"])
            .args(files)
            .current_dir(&dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.contains("z.rb:"))
            .map(String::from)
            .collect::<Vec<_>>()
    };
    let alone = run(&["z.rb"]);
    assert_eq!(alone.len(), 31, "{alone:?}");
    assert_eq!(run(&["."]), alone);
    let _ = fs::remove_dir_all(&dir);
}

/// Below 3.3 a file with a syntax error is reparsed with rewrites for the parser gem's lexer; the
/// per-token scans behind them stay linear on one long line (#316: half these sizes took 9.7 s and 4.3 s CPU).
#[test]
fn legacy_rewrites_stay_linear_on_one_line() {
    let cases = [
        ("dots.rb", format!("x = 1; {}\n)\n", "a.b; ".repeat(800_000))),
        ("data_markers.rb", format!("x = [{}]\n)\n", "__END__, ".repeat(400_000))),
    ];
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("rbcop-hostile-line-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  TargetRubyVersion: 3.2\n  NewCops: enable\n").unwrap();
    for (name, text) in cases {
        fs::write(dir.join(name), text).unwrap();
        let cpu0 = children_cpu();
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(["--only", "Lint/Syntax", "--format", "emacs", name])
            .current_dir(&dir)
            .output()
            .unwrap();
        let cpu = children_cpu() - cpu0;
        assert_eq!(out.status.code(), Some(1), "{name}: {}", String::from_utf8_lossy(&out.stderr));
        assert!(cpu < Duration::from_secs(10), "{name}: took {cpu:?} CPU");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// -A stays linear in the number of corrections: 100,000 correctable offenses (one per line)
/// took 58 s CPU (a whole-source line/column scan per offense, and a copy of every earlier edit
/// per merged correction); now well under a second.
#[test]
fn many_corrections_stay_linear() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("rbcop-hostile-ac-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join("t.rb"), "x \"a\"\n".repeat(100_000)).unwrap();
    let cpu0 = children_cpu();
    let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--force-default-config", "--only", "Style/StringLiterals", "-A", "--format", "emacs", "t.rb"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let cpu = children_cpu() - cpu0;
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(fs::read_to_string(dir.join("t.rb")).unwrap(), "x 'a'\n".repeat(100_000));
    assert!(cpu < Duration::from_secs(10), "took {cpu:?} CPU");
    let _ = fs::remove_dir_all(&dir);
}

/// A config mapping with many keys (#600): loading and merging look keys up through an index.
/// 80000 keys under one parameter and 40000 cop parameters (a warning each) took 53 s and 35 s,
/// mostly key scans; now about a second each.
#[test]
fn many_config_keys_stay_linear() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("rbcop-hostile-keys-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join("t.rb"), "x = 1\n").unwrap();
    let nested: String = (0..80_000).map(|k| format!("    K{k}: {k}\n")).collect();
    let params: String = (0..40_000).map(|k| format!("  K{k}: {k}\n")).collect();
    for (name, keys) in [("nested", format!("  X:\n{nested}")), ("params", params)] {
        let config = format!("AllCops:\n  SuggestExtensions: false\nLayout/LineLength:\n{keys}");
        fs::write(dir.join(".rubocop.yml"), config).unwrap();
        let cpu0 = children_cpu();
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(["--only", "Layout/LineLength", "--format", "emacs", "t.rb"])
            .current_dir(&dir)
            .output()
            .unwrap();
        let cpu = children_cpu() - cpu0;
        assert_eq!(out.status.code(), Some(0), "{name}");
        assert!(cpu < Duration::from_secs(10), "{name}: took {cpu:?} CPU");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Below 3.3 (the parser gem's lexer is modelled on Prism's tokens), per-line token work stays
/// linear: each `(file, --only, text)` takes well under a second; the budget catches a scan or
/// reparse per line.
#[test]
fn legacy_token_scans_stay_linear() {
    let cases = [
        // Layout/EndOfLine reparsed the whole file per heredoc line ending in an identifier (#317):
        // 48k lines took about 40 s.
        ("heredoc_ident.rb", "Layout/EndOfLine", format!("def bar; end\n{}", "foo <<~A, bar\n  x\nA\n".repeat(12_000))),
    ];
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("rbcop-hostile-legacy-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  TargetRubyVersion: 3.2\n  NewCops: enable\n").unwrap();
    for (name, only, text) in cases {
        fs::write(dir.join(name), text).unwrap();
        let cpu0 = children_cpu();
        let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .args(["--only", only, "--format", "emacs", name])
            .current_dir(&dir)
            .output()
            .unwrap();
        let cpu = children_cpu() - cpu0;
        assert!(matches!(out.status.code(), Some(0 | 1)), "{name}: {}", String::from_utf8_lossy(&out.stderr));
        assert!(cpu < Duration::from_secs(10), "{name}: took {cpu:?} CPU");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Below 3.3 a constant starting with a non-ASCII capital gets a lowercase stand-in absent from
/// the file: the candidates were each searched for in the whole text, for every such constant when
/// none was free (8,000 lines took 10 s, #300). Now the file's characters are collected once.
#[test]
fn non_ascii_capital_stand_ins_stay_linear() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("rbcop-hostile-capital-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join(".rubocop.yml"), "AllCops:\n  TargetRubyVersion: 3.2\n  NewCops: disable\n").unwrap();
    // Every 2-byte lowercase letter occurs, so `Ж` has no stand-in at all.
    let lower: String = (0x80..0x800).filter_map(char::from_u32).filter(|c| c.is_lowercase()).collect();
    fs::write(dir.join("t.rb"), format!("{}# {lower}\n", "Жa\n".repeat(40_000))).unwrap();
    let cpu0 = children_cpu();
    let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--only", "Lint/Syntax", "--format", "emacs", "t.rb"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let cpu = children_cpu() - cpu0;
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
    assert!(cpu < Duration::from_secs(10), "took {cpu:?} CPU");
    let _ = fs::remove_dir_all(&dir);
}

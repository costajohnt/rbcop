//! `--mcp` transcripts: rbcop, run in a copy of tests/mcp/*/project, must answer input.jsonl with
//! exactly the lines `rubocop --mcp` wrote (expected.jsonl and status, recorded by scripts/mcp.sh).

mod common;

use std::io::Write;
use std::path::Path;
use std::process::Stdio;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Deliberate difference: RuboCop's tools read and write any path the client names (and
/// `rubocop_explain` reads the configuration of any); rbcop's refuse one that resolves outside the
/// working directory (absolute, `..`, or through a symlink).
#[test]
fn mcp_tools_stay_inside_the_project() {
    use std::os::unix::fs::symlink;
    let base = std::env::temp_dir().join(format!("rbcop-mcp-contain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (project, outside) = (base.join("project"), base.join("outside"));
    std::fs::create_dir_all(project.join("lib")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let base = base.canonicalize().unwrap();
    let (project, outside) = (base.join("project"), base.join("outside"));
    std::fs::write(project.join("Gemfile"), "").unwrap();
    std::fs::write(project.join("lib/a.rb"), "puts( 1)\n").unwrap();
    std::fs::write(outside.join("o.rb"), "puts( 9)\n").unwrap();
    symlink(outside.join("o.rb"), project.join("lib/evil.rb")).unwrap();
    let o = outside.join("o.rb").display().to_string();
    let call = |id: u32, tool: &str, args: &str| {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"rubocop_{tool}\",\"arguments\":{args}}}}}\n"
        )
    };
    let refused = [
        call(1, "inspection", &format!("{{\"path\":\"{o}\"}}")),
        call(2, "inspection", "{\"path\":\"../outside/o.rb\"}"),
        call(3, "inspection", "{\"path\":\"lib/evil.rb\"}"),
        call(4, "inspection", "{\"path\":\"../outside/x.rb\",\"source_code\":\"x\\n\"}"),
        call(5, "autocorrection", "{\"path\":\"lib/evil.rb\",\"safety\":false}"),
        call(6, "autocorrection", "{\"path\":\"lib/evil.rb\",\"source_code\":\"pwned\\n\",\"safety\":true}"),
        call(7, "autocorrection", "{\"path\":\"../outside/new.rb\",\"source_code\":\"pwned\\n\",\"safety\":true}"),
        call(8, "autocorrection", &format!("{{\"path\":\"{o}\",\"safety\":true}}")),
        call(9, "autocorrection", "{\"path\":\"lib\",\"safety\":true}"),
        call(10, "autocorrection", "{\"safety\":true}"),
        call(11, "explain", "{\"cop_names\":[\"Style/StringLiterals\"],\"path\":\"../outside/o.rb\"}"),
    ];
    // Inside the project a new file is still written.
    let input = refused.concat()
        + &call(12, "autocorrection", "{\"path\":\"lib/new.rb\",\"source_code\":\"puts( 2)\\n\",\"safety\":true}");
    let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .arg("--mcp")
        .current_dir(&project)
        .env("PWD", &project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 12, "{out}");
    for line in &lines[..11] {
        assert!(line.contains("\"isError\":true") && line.contains("outside the project"), "{line}");
    }
    assert!(lines[11].contains("\"isError\":false"), "{}", lines[11]);
    assert_eq!(std::fs::read_to_string(project.join("lib/new.rb")).unwrap(), "puts(2)\n");
    assert_eq!(std::fs::read_to_string(outside.join("o.rb")).unwrap(), "puts( 9)\n");
    assert!(!outside.join("new.rb").exists());
    assert!(std::fs::symlink_metadata(project.join("lib/evil.rb")).unwrap().file_type().is_symlink());
    std::fs::remove_dir_all(&base).unwrap();
}

/// The tools' `changed` (Scope#select_files, ChangedFiles) in a throwaway git repository: b.rb
/// modified since the commit, c.rb untracked, a.rb unchanged. tests/mcp_changed.jsonl is what
/// `rubocop --mcp` 1.92.1 answered to the same requests in the same repository (`$ROOT` for it).
#[test]
fn mcp_changed_selects_the_files_git_reports() {
    let base = std::env::temp_dir().join(format!("rbcop-mcp-changed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("repo");
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let gitconfig = base.join("gitconfig");
    std::fs::write(&gitconfig, "[user]\n\tname = t\n\temail = t@example.invalid\n").unwrap();
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(dir.join(".rubocop.yml"), "AllCops:\n  NewCops: enable\n  SuggestExtensions: false\n").unwrap();
    std::fs::write(dir.join("a.rb"), "puts( 1)\n").unwrap();
    std::fs::write(dir.join("b.rb"), "puts( 2)\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);
    std::fs::write(dir.join("b.rb"), "puts( 3)\n").unwrap();
    std::fs::write(dir.join("c.rb"), "puts( 4)\n").unwrap();
    let call = |id: u32, tool: &str, args: &str| {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"rubocop_{tool}\",\"arguments\":{args}}}}}\n"
        )
    };
    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"t\",\"version\":\"1\"}}}\n".to_string()
        + &call(2, "inspection", "{\"changed\":true,\"only\":[\"Layout\"]}")
        + &call(3, "inspection", "{\"changed\":\"HEAD\",\"path\":\"a.rb\"}")
        + &call(4, "inspection", "{\"changed\":\"nope\"}")
        + &call(5, "autocorrection", "{\"changed\":true,\"safety\":true,\"dry_run\":true,\"only\":[\"Layout\"]}");
    let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .arg("--mcp")
        .current_dir(&dir)
        .env("PWD", &dir)
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
    let _ = std::fs::remove_dir_all(&base);
    let expected =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mcp_changed.jsonl")).unwrap();
    assert_eq!(out.replace(&dir.display().to_string(), "$ROOT"), expected);
}

#[test]
fn mcp_transcripts_match_rubocop() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mcp");
    let mut names: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    assert!(!names.is_empty());
    for dir in names {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        // The autocorrection tool writes files: run in a scratch copy of the project.
        let scratch = std::env::temp_dir().join(format!("rbcop-mcp-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        copy_dir(&dir.join("project"), &scratch);
        let project = scratch.canonicalize().unwrap();
        let ids = project.display().to_string();
        let input = std::fs::read_to_string(dir.join("input.jsonl")).unwrap().replace("$ROOT", &ids);
        let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .arg("--mcp")
            .current_dir(&project)
            .env("PWD", &project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        let _ = std::fs::remove_dir_all(&scratch);
        let actual: Vec<String> =
            String::from_utf8(out.stdout).unwrap().lines().map(|l| l.replace(&ids, "$ROOT")).collect();
        let expected: Vec<String> =
            std::fs::read_to_string(dir.join("expected.jsonl")).unwrap().lines().map(String::from).collect();
        for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(a, e, "{name}: line {}", i + 1);
        }
        assert_eq!(actual.len(), expected.len(), "{name}: line count");
        let status = std::fs::read_to_string(dir.join("status")).unwrap();
        assert_eq!(out.status.code(), status.trim().parse().ok(), "{name}: exit status");
    }
}

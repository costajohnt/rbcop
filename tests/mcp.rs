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
/// Runs `rbcop --mcp` in `dir` on `requests` (one JSON-RPC line each), its answers one per line.
fn mcp(dir: &Path, envs: &[(&str, &Path)], requests: &[String]) -> Vec<String> {
    let mut cmd = common::command(env!("CARGO_BIN_EXE_rbcop"));
    cmd.arg("--mcp").current_dir(dir).env("PWD", dir);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(requests.concat().as_bytes()).unwrap();
    String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap().lines().map(String::from).collect()
}

fn tool_call(id: u32, tool: &str, args: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"tools/call\",\"params\":{{\"name\":\"rubocop_{tool}\",\"arguments\":{args}}}}}\n"
    )
}

/// Deliberate difference: the tools never read or write under version control metadata (`.git`,
/// `.hg`, `.svn`, `.jj`, in any case), where a written `.git/config` could make git run a command.
#[test]
fn mcp_tools_refuse_version_control_metadata() {
    let dir = std::env::temp_dir().join(format!("rbcop-mcp-vcs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    let dir = dir.canonicalize().unwrap();
    std::fs::write(dir.join(".git/config"), "[core]\n").unwrap();
    std::fs::write(dir.join(".git/x.rb"), "puts( 1)\n").unwrap();
    let requests = [
        tool_call(
            1,
            "autocorrection",
            r#"{"path":".git/config","source_code":"[core]\n\tfsmonitor = x\n","safety":true}"#,
        ),
        tool_call(2, "autocorrection", r#"{"path":".git/x.rb","safety":true}"#),
        tool_call(3, "autocorrection", r#"{"path":"./.git/new.rb","source_code":"x\n","safety":true}"#),
        tool_call(4, "autocorrection", r#"{"path":".Hg/hgrc","source_code":"x\n","safety":true}"#),
        tool_call(5, "inspection", r#"{"path":".git"}"#),
        tool_call(6, "inspection", r#"{"path":".svn/a.rb","source_code":"x\n"}"#),
        tool_call(7, "explain", r#"{"cop_names":["Style/StringLiterals"],"path":".jj/a.rb"}"#),
    ];
    let lines = mcp(&dir, &[], &requests);
    assert_eq!(lines.len(), requests.len(), "{lines:?}");
    for line in &lines {
        assert!(line.contains("\"isError\":true") && line.contains("inside version control metadata"), "{line}");
    }
    assert_eq!(std::fs::read_to_string(dir.join(".git/config")).unwrap(), "[core]\n");
    assert_eq!(std::fs::read_to_string(dir.join(".git/x.rb")).unwrap(), "puts( 1)\n");
    assert!(!dir.join(".git/new.rb").exists() && !dir.join(".Hg").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Deliberate difference: `--changed` and the MCP `changed` scope run git with the repository's
/// command hooks off, so a planted `core.fsmonitor` (which plain `git diff` runs) never runs.
#[test]
fn changed_never_runs_a_planted_fsmonitor() {
    let base = std::env::temp_dir().join(format!("rbcop-fsmonitor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("repo");
    std::fs::create_dir_all(&dir).unwrap();
    let (base, dir) = (base.canonicalize().unwrap(), dir.canonicalize().unwrap());
    let gitconfig = base.join("gitconfig");
    std::fs::write(&gitconfig, "[user]\n\tname = t\n\temail = t@example.invalid\n").unwrap();
    let marker = base.join("ran");
    let hook = base.join("hook.sh");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .output();
        assert!(out.unwrap().status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(dir.join("a.rb"), "puts( 1)\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);
    std::fs::write(dir.join("a.rb"), "puts( 2)\n").unwrap();
    git(&["config", "core.fsmonitor", &hook.display().to_string()]);
    // Plain git runs the hook: the planted config is live.
    git(&["diff", "--name-only"]);
    assert!(marker.exists(), "git itself did not run the fsmonitor hook; the test proves nothing");
    std::fs::remove_file(&marker).unwrap();
    let cli = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(["--changed", "--only", "Layout/SpaceInsideParens", "-f", "e"])
        .current_dir(&dir)
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&cli.stdout).contains("a.rb:1:6"), "{cli:?}");
    let lines = mcp(&dir, &[("GIT_CONFIG_GLOBAL", &gitconfig)], &[tool_call(1, "inspection", r#"{"changed":true}"#)]);
    assert!(lines[0].contains("a.rb") && lines[0].contains("\"isError\":false"), "{lines:?}");
    assert!(!marker.exists(), "rbcop ran the planted core.fsmonitor");
    std::fs::remove_dir_all(&base).unwrap();
}

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

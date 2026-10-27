//! `--lsp` transcripts: rbcop must answer tests/lsp/*/input.jsonl with exactly the messages
//! `rubocop --lsp` sent (expected.jsonl and status, recorded by scripts/lsp.sh).

mod common;

use std::io::Write;
use std::path::Path;
use std::process::Stdio;

/// The bodies of the Content-Length framed messages in `out`, each checked against its header.
fn bodies(mut out: &[u8]) -> Vec<String> {
    let mut list = Vec::new();
    while !out.is_empty() {
        let sep = out.windows(4).position(|w| w == b"\r\n\r\n").expect("a header");
        let header = std::str::from_utf8(&out[..sep]).unwrap();
        let n: usize = header.strip_prefix("Content-Length: ").expect("Content-Length").parse().unwrap();
        list.push(String::from_utf8(out[sep + 4..sep + 4 + n].to_vec()).unwrap());
        out = &out[sep + 4 + n..];
    }
    list
}

#[test]
fn lsp_transcripts_match_rubocop() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lsp");
    let mut names: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    assert!(!names.is_empty());
    for dir in names {
        let project = dir.join("project").canonicalize().unwrap();
        let ids = project.display().to_string();
        let mut input = Vec::new();
        for line in std::fs::read_to_string(dir.join("input.jsonl")).unwrap().lines() {
            let m = line.replace("$ROOT", &ids);
            write!(input, "Content-Length: {}\r\n\r\n{m}", m.len()).unwrap();
        }
        let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
            .arg("--lsp")
            .current_dir(&project)
            .env("PWD", &project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // The server may exit before reading everything (shutdown): ignore a broken pipe.
        let _ = child.stdin.take().unwrap().write_all(&input);
        let out = child.wait_with_output().unwrap();
        let actual: Vec<String> = bodies(&out.stdout).iter().map(|b| b.replace(&ids, "$ROOT")).collect();
        let expected: Vec<String> =
            std::fs::read_to_string(dir.join("expected.jsonl")).unwrap().lines().map(String::from).collect();
        let name = dir.file_name().unwrap().to_string_lossy();
        for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(a, e, "{name}: message {}", i + 1);
        }
        assert_eq!(actual.len(), expected.len(), "{name}: message count");
        let status = std::fs::read_to_string(dir.join("status")).unwrap();
        assert_eq!(out.status.code(), status.trim().parse().ok(), "{name}: exit status");
    }
}

/// AllCops/UseProjectIndex under `--lsp` (#675): the index is built from disk at the first run and
/// kept while files change on disk, until `textDocument/didSave` or a watched file's change drops
/// it. What `rubocop --lsp` 1.92.1 (rubydex 0.4.1) publishes for b.rb after each step: the two
/// offenses of lib.rb's first version, again after lib.rb is rewritten, none after didSave
/// (rebuilt from the second version), and both again after lib.rb is restored and reported by
/// didChangeWatchedFiles.
#[test]
fn lsp_rebuilds_the_project_index_on_save() {
    use std::io::{BufRead, BufReader, Read};
    let dir = std::env::temp_dir().join(format!("rbcop-lsp-index-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let lib = |arity: &str| {
        format!(
            "# frozen_string_literal: true\n\n# Foo\nclass Foo\n  def self.[]({arity}); end\n\n  def self.bar({arity}); end\nend\n"
        )
    };
    std::fs::write(dir.join("Gemfile"), "source \"https://rubygems.org\"\n").unwrap();
    std::fs::write(
        dir.join(".rubocop.yml"),
        "AllCops:\n  UseProjectIndex: true\n  NewCops: enable\n  SuggestExtensions: false\n",
    )
    .unwrap();
    std::fs::write(dir.join("lib.rb"), lib("a")).unwrap();
    let mut child = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .arg("--lsp")
        .current_dir(&dir)
        .env("PWD", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let root = format!("file://{}", dir.display());
    let text = r"# frozen_string_literal: true\n\nFoo[1, 2] += 1\nFoo.bar(1, 2)\n";
    let mut send = |m: String| write!(input, "Content-Length: {}\r\n\r\n{m}", m.len()).unwrap();
    let mut read = || {
        let mut n = 0;
        loop {
            let mut line = String::new();
            output.read_line(&mut line).unwrap();
            if let Some(len) = line.strip_prefix("Content-Length: ") {
                n = len.trim().parse().unwrap();
            } else if line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0; n];
        output.read_exact(&mut body).unwrap();
        String::from_utf8(body).unwrap().matches("Wrong number of arguments").count()
    };
    let change = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{root}/b.rb","version":2}},"contentChanges":[{{"text":"{text}"}}]}}}}"#
    );
    send(format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"processId":null,"rootUri":"{root}","capabilities":{{}}}}}}"#
    ));
    read();
    send(format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{root}/b.rb","languageId":"ruby","version":1,"text":"{text}"}}}}}}"#
    ));
    let mut counts = vec![read()];
    std::fs::write(dir.join("lib.rb"), lib("a, b")).unwrap();
    send(change.clone());
    counts.push(read());
    send(format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didSave","params":{{"textDocument":{{"uri":"{root}/b.rb"}}}}}}"#
    ));
    send(change.clone());
    counts.push(read());
    std::fs::write(dir.join("lib.rb"), lib("a")).unwrap();
    send(format!(
        r#"{{"jsonrpc":"2.0","method":"workspace/didChangeWatchedFiles","params":{{"changes":[{{"uri":"{root}/lib.rb","type":2}}]}}}}"#
    ));
    send(change);
    counts.push(read());
    send(r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#.to_string());
    read();
    drop(input);
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(counts, [2, 2, 0, 2]);
}

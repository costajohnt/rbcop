//! Style/BlockDelimiters move_comment_before_block when the chain ends after the comment: parser's
//! Range#join takes min begin / max end, so the pre-comment range runs from the comment to the chain
//! end (this used to panic on a reversed slice). Expected output is RuboCop 1.91.0's.

mod common;

use std::fs;

#[test]
fn comment_before_chain_end_moves_above_block() {
    let dir = std::env::temp_dir().join(format!("rbcop-bd-chain-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Gemfile"), "").unwrap();
    fs::write(dir.join("t.rb"), "expect {\n  foo }.not_to( # does not log\n  bar\n)\n").unwrap();
    let out = common::command(env!("CARGO_BIN_EXE_rbcop"))
        .args(["-A", "--force-default-config", "--only", "Style/BlockDelimiters", "-f", "emacs", "t.rb"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(!stderr.contains("error occurred"), "{stderr}");
    assert!(stdout.contains("t.rb:1:8: C: [Corrected] Style/BlockDelimiters:"), "{stdout}");
    let fixed = fs::read_to_string(dir.join("t.rb")).unwrap();
    assert_eq!(fixed, "# does not log\nexpect do\n  foo end.not_to(   bar\n)\n\n");
    let _ = fs::remove_dir_all(&dir);
}

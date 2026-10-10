//! Shared by the integration tests: a Command whose environment cannot change rbcop's output.

use std::ffi::OsStr;
use std::process::Command;

/// `program` with every variable rbcop reads (src/main.rs, src/config, src/formatter) cleared, and
/// HOME pointed at a directory that does not exist so no user-level .rubocop.yml is picked up.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    for var in [
        "RUBOCOP_OPTS",
        "RUBOCOP_TARGET_RUBY_VERSION",
        "CLICOLOR_FORCE",
        "TERM",
        "RUBY_PROGRESS_BAR_LENGTH",
        "RBCOP_DEBUG",
        "XDG_CONFIG_HOME",
    ] {
        cmd.env_remove(var);
    }
    cmd.env("HOME", "/nonexistent-rbcop-test-home");
    cmd
}

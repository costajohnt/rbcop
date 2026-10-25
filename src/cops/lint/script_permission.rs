use crate::prelude::*;
use std::os::unix::fs::PermissionsExt;

pub const NAME: &str = "Lint/ScriptPermission";
pub static COP: ScriptPermission = ScriptPermission;
pub struct ScriptPermission;

/// `@options.key?(:stdin)`: the cop is skipped for `-s/--stdin` (and `--lsp`, whose runs are stdin runs).
/// ponytail: read from argv because Cx does not carry CLI options; misses a combined `-sX` flag.
fn stdin_mode() -> bool {
    std::env::args().any(|a| a == "-s" || a == "--stdin" || a.starts_with("--stdin="))
}

impl Cop for ScriptPermission {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        if !cx.src.text.starts_with("#!") || stdin_mode() || crate::helpers::lsp::serving() {
            return;
        }
        let path = std::path::Path::new(cx.src.path);
        // ponytail: File.stat(..).executable? checks the bit for the effective user's class; any x bit
        // counts here.
        let Ok(meta) = std::fs::metadata(path) else { return };
        if meta.permissions().mode() & 0o111 != 0 {
            return;
        }
        let Some(comment) = cx.src.comments.first() else { return };
        let (s, e) = (comment.start, comment.end);
        let base = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let autocorrect = cx.fixes.autocorrect;
        let p = path.to_path_buf();
        // The block chmods +x but makes no source edit, so the offense is never [Correctable].
        cx.add_fix(s, e, format!("Script file {base} doesn't have execute permission."), |_| {
            if autocorrect {
                let mode = meta.permissions().mode();
                // FileUtils.chmod('+x'): no "who" means all three execute bits.
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode | 0o111));
            }
        });
    }
}

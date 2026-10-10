use crate::prelude::*;

pub const NAME: &str = "Lint/RequireRelativeSelfPath";
pub static COP: RequireRelativeSelfPath = RequireRelativeSelfPath;
pub struct RequireRelativeSelfPath;

impl Cop for RequireRelativeSelfPath {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "require_relative" || is_csend(&call) {
            return;
        }
        let Some(first) = send_args(&call).into_iter().next() else { return };
        // ponytail: only `str` values; a dstr's `value` is not compared.
        let Some(s) = first.as_string_node() else { return };
        let value = s.unescaped();
        let Some(base) = std::path::Path::new(cx.src.path).file_name().and_then(|f| f.to_str()) else { return };
        // File.extname(path) == '.rb' (a bare `.rb` has no extension).
        let Some(stem) = base.strip_suffix(".rb").filter(|s| !s.is_empty()) else { return };
        if value != stem.as_bytes() && value != base.as_bytes() {
            return;
        }
        let (start, end) = send_range(&call);
        let (ws, we) = helpers::whole_lines::range_by_whole_lines(cx.src.text, start, end, true);
        cx.add_fix(start, end, "Remove the `require_relative` that requires itself.", |c| c.remove(ws, we));
    }
}

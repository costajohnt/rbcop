use crate::prelude::*;
use helpers::parser_parent::{is_top_const, parser_parent_index};

pub const NAME: &str = "Style/StringHashKeys";
pub static COP: StringHashKeys = StringHashKeys;
pub struct StringHashKeys;

/// `(send recv name ...)` with `recv` checked by `recv_ok`.
fn send_matches(node: &Node, names: &[&str], recv_ok: impl Fn(Option<Node>) -> bool) -> bool {
    node.as_call_node().is_some_and(|c| !is_csend(&c) && names.contains(&method_name(&c)) && recv_ok(c.receiver()))
}

impl Cop for StringHashKeys {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ASSOC_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let pair = node.as_assoc_node().unwrap();
        let key = pair.key();
        let Some(s) = key.as_string_node() else { return };
        if is_parser_dstr(&key) || is_heredoc(&key) {
            return;
        }
        let Ok(content) = std::str::from_utf8(s.unescaped()) else { return };
        if receive_environments_method(cx.ancestors) {
            return;
        }
        let sym = helpers::ruby_string::symbol_inspect(content.as_bytes());
        let loc = key.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        cx.add_fix(start, end, "Prefer symbols instead of strings as hash keys.", |c| c.replace(start, end, &sym));
    }
}

fn receive_environments_method(anc: &[Node]) -> bool {
    let up = |i: usize| parser_parent_index(anc, i);
    let Some(hash) = up(anc.len()) else { return false };
    let Some(p2) = up(hash) else { return false };
    let const_recv = |names: &'static [&'static str]| {
        move |r: Option<Node>| r.is_some_and(|r| names.iter().any(|n| is_top_const(&r, n)))
    };
    let n = &anc[p2];
    if send_matches(n, &["popen"], const_recv(&["IO"]))
        || send_matches(n, &["capture2", "capture2e", "capture3", "popen2", "popen2e", "popen3"], const_recv(&["Open3"]))
        || send_matches(n, &["spawn", "system"], |r| r.is_none_or(|r| is_top_const(&r, "Kernel")))
        || send_matches(n, &["gsub", "gsub!"], |_| true)
    {
        return true;
    }
    up(p2).is_some_and(|p3| {
        send_matches(
            &anc[p3],
            &["pipeline", "pipeline_r", "pipeline_rw", "pipeline_start", "pipeline_w"],
            const_recv(&["Open3"]),
        )
    })
}

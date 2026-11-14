use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidExtendTSigHelpersInShims";
pub static COP: ForbidExtendTSigHelpersInShims = ForbidExtendTSigHelpersInShims;
pub struct ForbidExtendTSigHelpersInShims;

/// `(const (const nil? :T) {:Sig :Helpers})`
fn t_sig_or_helpers(node: &Node) -> bool {
    node.as_constant_path_node().is_some_and(|p| {
        p.name().is_some_and(|n| matches!(n.as_slice(), b"Sig" | b"Helpers"))
            && p.parent().and_then(|t| t.as_constant_read_node()).is_some_and(|t| t.name().as_slice() == b"T")
    })
}

impl Cop for ForbidExtendTSigHelpersInShims {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || call.name().as_slice() != b"extend" {
            return;
        }
        if !matches!(send_args(&call).as_slice(), [a] if t_sig_or_helpers(a)) {
            return;
        }
        let (s, e) = send_range(&call);
        let (ls, le) = helpers::whole_lines::range_by_whole_lines(cx.src.text, s, e, true);
        cx.add_fix(s, e, "Extending T::Sig or T::Helpers in a shim is unnecessary", |c| c.remove(ls, le));
    }
}

use crate::prelude::*;

pub const NAME: &str = "Style/BarePercentLiterals";
pub static COP: BarePercentLiterals = BarePercentLiterals;
pub struct BarePercentLiterals;

impl Cop for BarePercentLiterals {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE, kind::INTERPOLATED_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let opening = match node {
            Node::StringNode { .. } => node.as_string_node().unwrap().opening_loc(),
            _ => node.as_interpolated_string_node().unwrap().opening_loc(),
        };
        let Some(open) = opening else { return };
        let source = text(&open);
        if is_heredoc(node) {
            return;
        }
        let (good, bad) = if cx.cfg.str("EnforcedStyle") == Some("percent_q") {
            let b = source.as_bytes();
            if !(b.len() > 1 && b[0] == b'%' && !(b[1].is_ascii_alphanumeric() || b[1] == b'_')) {
                return;
            }
            ("Q", "")
        } else if source.starts_with("%Q") {
            ("", "Q")
        } else {
            return;
        };
        let (s, e) = (open.start_offset(), open.end_offset());
        let replacement = match source.strip_prefix("%Q") {
            Some(rest) => format!("%{rest}"),
            None => format!("%Q{}", &source[1..]),
        };
        cx.add_fix(s, e, format!("Use `%{good}` instead of `%{bad}`."), |c| c.replace(s, e, &replacement));
    }
}

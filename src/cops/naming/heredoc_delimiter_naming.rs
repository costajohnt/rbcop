use crate::helpers::heredoc;
use crate::prelude::*;

pub const NAME: &str = "Naming/HeredocDelimiterNaming";
pub static COP: HeredocDelimiterNaming = HeredocDelimiterNaming;
pub struct HeredocDelimiterNaming;

impl Cop for HeredocDelimiterNaming {
    fn kinds(&self) -> &'static [usize] {
        heredoc::KINDS
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some((open, end, empty)) = heredoc::parts(node) else { return };
        let delim = heredoc::delimiter_string(&cx.src.text[open.0..open.1]);
        // Ruby `\w` is ASCII-only
        let meaningful = delim.bytes().any(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !cx.cfg.regexes("ForbiddenDelimiters").iter().any(|r| r.is_match(delim));
        if !meaningful {
            let (s, e) = if empty { open } else { end };
            cx.add(s, e, "Use meaningful heredoc delimiters.");
        }
    }
}

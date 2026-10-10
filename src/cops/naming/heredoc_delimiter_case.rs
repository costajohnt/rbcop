use crate::helpers::heredoc;
use crate::prelude::*;

pub const NAME: &str = "Naming/HeredocDelimiterCase";
pub static COP: HeredocDelimiterCase = HeredocDelimiterCase;
pub struct HeredocDelimiterCase;

impl Cop for HeredocDelimiterCase {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        heredoc::KINDS
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(((os, oe), (es, ee), _)) = heredoc::parts(node) else { return };
        let upper = cx.cfg.str("EnforcedStyle").unwrap_or("uppercase") == "uppercase";
        let fix = |s: &str| if upper { s.to_uppercase() } else { s.to_lowercase() };
        let opening = &cx.src.text[os..oe];
        let delim = heredoc::delimiter_string(opening);
        if delim == fix(delim) {
            return;
        }
        let (new_open, new_end) = (fix(opening), fix(delim));
        let style = if upper { "uppercase" } else { "lowercase" };
        cx.add_fix(es, ee, format!("Use {style} heredoc delimiters."), |c| {
            c.replace(os, oe, &new_open);
            c.replace(es, ee, &new_end);
        });
    }
}

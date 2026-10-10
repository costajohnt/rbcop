use crate::prelude::*;

pub const NAME: &str = "Naming/AsciiIdentifiers";
pub static COP: AsciiIdentifiers = AsciiIdentifiers;
pub struct AsciiIdentifiers;

impl Cop for AsciiIdentifiers {
    fn on_file(&self, cx: &mut Cx) {
        let constants = cx.cfg.bool("AsciiConstants").unwrap_or(false);
        let src = cx.src;
        for t in helpers::parser_tokens::translated(src) {
            let identifier = t.kind == sys::PM_TOKEN_IDENTIFIER;
            if !identifier && !(constants && t.kind == sys::PM_TOKEN_CONSTANT) {
                continue;
            }
            let text = src.slice(t.start, t.end);
            // first run of non-ascii characters
            let Some(first) = text.find(|c: char| !c.is_ascii()) else { continue };
            let len = text[first..].find(|c: char| c.is_ascii()).unwrap_or(text.len() - first);
            let msg = if identifier {
                "Use only ascii symbols in identifiers."
            } else {
                "Use only ascii symbols in constants."
            };
            cx.add(t.start + first, t.start + first + len, msg);
        }
    }
}

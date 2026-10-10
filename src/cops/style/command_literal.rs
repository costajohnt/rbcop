use crate::prelude::*;

pub const NAME: &str = "Style/CommandLiteral";
pub static COP: CommandLiteral = CommandLiteral;
pub struct CommandLiteral;

impl Cop for CommandLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::X_STRING_NODE, kind::INTERPOLATED_X_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_heredoc(node) {
            return;
        }
        let (open, close) = match node.as_x_string_node() {
            Some(x) => (x.opening_loc(), x.closing_loc()),
            None => {
                let x = node.as_interpolated_x_string_node().unwrap();
                (x.opening_loc(), x.closing_loc())
            }
        };
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        let body = cx.src.slice(open.end_offset(), close.start_offset());
        let has_backtick = body.contains('`');
        let disallowed = !cx.cfg.bool("AllowInnerBackticks").unwrap_or(false) && has_backtick;
        let multiline = cx.src.line_of(s) != cx.src.line_of(e);
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("backticks");
        let backtick = open.as_slice() == b"`";
        let allowed = if backtick {
            match style {
                "backticks" => !disallowed,
                "mixed" => !multiline && !disallowed,
                _ => false,
            }
        } else {
            match style {
                "backticks" => disallowed,
                "mixed" => multiline || disallowed,
                _ => true,
            }
        };
        if allowed {
            return;
        }
        let msg = if backtick { "Use `%x` around command string." } else { "Use backticks around command string." };
        let delims = cx.config.for_cop("Style/PercentLiteralDelimiters");
        let preferred = delims.map("PreferredDelimiters");
        let delim = preferred
            .and_then(|m| m.str("%x").or_else(|| m.str("default")))
            .unwrap_or("()")
            .to_string();
        let mut chars = delim.chars();
        let (first, last) = if backtick {
            (format!("%x{}", chars.next().map(String::from).unwrap_or_default()), chars.next().map(String::from).unwrap_or_default())
        } else {
            ("`".to_string(), "`".to_string())
        };
        let (os, oe, cs, ce) = (open.start_offset(), open.end_offset(), close.start_offset(), close.end_offset());
        cx.add_fix(s, e, msg, |c| {
            if has_backtick {
                return;
            }
            c.replace(os, oe, &first);
            c.replace(cs, ce, &last);
        });
    }
}

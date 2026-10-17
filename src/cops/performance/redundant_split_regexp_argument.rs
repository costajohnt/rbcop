use crate::helpers::regexp_metacharacter::literal_before_final_newline;
use crate::prelude::*;

pub const NAME: &str = "Performance/RedundantSplitRegexpArgument";
pub static COP: RedundantSplitRegexpArgument = RedundantSplitRegexpArgument;
pub struct RedundantSplitRegexpArgument;

const STR_SPECIAL_CHARS: &[&str] = &["\\n", "\\\"", "\\'", "\\\\", "\\t", "\\b", "\\f", "\\r"];

impl Cop for RedundantSplitRegexpArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call !nil? :split $regexp)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "split" || call.receiver().is_none() {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        // DETERMINISTIC_REGEX on the regexp's whole source: only a plain `/.../` can match.
        let Some(re) = arg.as_regular_expression_node() else { return };
        if text(&re.closing_loc()).contains('i') {
            return;
        }
        let content = text(&re.content_loc());
        if content == " " || !literal_before_final_newline(node_text(arg)) {
            return;
        }
        let replacement = format!("\"{}\"", replacement(content));
        let loc = arg.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        cx.add_fix(start, end, "Use string as argument instead of regexp.", |c| c.replace(start, end, &replacement));
    }
}

fn replacement(content: &str) -> String {
    let mut pieces: Vec<String> = Vec::new();
    let mut pending = false;
    for c in content.chars() {
        if !pending && c == '\\' {
            pending = true;
        } else {
            pieces.push(if std::mem::take(&mut pending) { format!("\\{c}") } else { c.to_string() });
        }
    }
    pieces
        .into_iter()
        .map(|p| if STR_SPECIAL_CHARS.contains(&p.as_str()) { p } else { p.replace('\\', "") })
        .collect()
}

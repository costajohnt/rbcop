use crate::prelude::*;

pub const NAME: &str = "Performance/RedundantStringChars";
pub static COP: RedundantStringChars = RedundantStringChars;
pub struct RedundantStringChars;

impl Cop for RedundantStringChars {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send $(send !nil? :chars) $_ $...)
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if is_csend(&call) || !matches!(method, "[]" | "slice" | "first" | "last" | "take" | "length" | "size" | "empty?") {
            return;
        }
        let Some(chars) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if method_name(&chars) != "chars"
            || is_csend(&chars)
            || chars.receiver().is_none()
            || !send_args(&chars).is_empty()
            || chars.block().is_some()
        {
            return;
        }
        let args = send_args(&call);
        if (method == "last" && !args.is_empty()) || args.len() == 2 {
            return;
        }
        let joined = || args.iter().map(|a| node_text(a)).collect::<Vec<_>>().join(", ");
        let good = match method {
            "slice" => format!("[{}].chars", joined()),
            "[]" | "first" => match args.first() {
                Some(a) if a.as_range_node().is_some() => format!("[{}].chars", joined()),
                Some(a) if method == "first" => format!("[0...{}].chars", node_text(a)),
                Some(a) => format!("[{}]", node_text(a)),
                None => "[0]".to_string(),
            },
            "last" => "[-1]".to_string(),
            // `args.first.source` on no argument raises: no offense.
            "take" => match args.first() {
                Some(a) => format!("[0...{}].chars", node_text(a)),
                None => return,
            },
            _ => format!(".{method}"),
        };
        let bad = if method == "[]" {
            format!("chars[{}]", joined())
        } else if !args.is_empty() {
            format!("chars.{method}({})", joined())
        } else {
            format!("chars.{method}")
        };
        let start = chars.message_loc().unwrap().start_offset();
        let end = send_range(&call).1;
        let dot = chars.call_operator_loc().unwrap().start_offset();
        cx.add_fix(start, end, format!("Use `{good}` instead of `{bad}`."), |c| c.replace(dot, end, &good));
    }
}

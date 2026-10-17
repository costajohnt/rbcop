use crate::prelude::*;

pub const NAME: &str = "Performance/UnfreezeString";
pub static COP: UnfreezeString = UnfreezeString;
pub struct UnfreezeString;

/// Parser `str` or `dstr`.
fn string(node: &Node) -> bool {
    matches!(kind::of(node), kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE | kind::SOURCE_FILE_NODE)
}

impl Cop for UnfreezeString {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.3, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let args = send_args(&call);
        let hit = match method_name(&call) {
            // (send {str dstr} :dup), below Ruby 3.3
            "dup" => args.is_empty() && string(&recv) && cx.config.target_ruby <= 3.2,
            // (send (const nil? :String) :new {str dstr}?)
            "new" => {
                recv.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"String")
                    && match args.as_slice() {
                        [] => true,
                        [a] => string(a),
                        _ => false,
                    }
            }
            _ => false,
        };
        if !hit {
            return;
        }
        let value = if node_text(&recv) == "String" && method_name(&call) == "new" {
            args.first().map_or("''", |a| node_text(a))
        } else {
            node_text(&recv)
        };
        let mut replacement = format!("+{value}");
        let parent_send = helpers::parser_parent::parser_parent(cx.ancestors)
            .and_then(|p| p.as_call_node())
            .is_some_and(|p| !is_csend(&p));
        if parent_send {
            replacement = format!("({replacement})");
        }
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, "Use unary plus to get an unfrozen string literal.", |c| c.replace(start, end, &replacement));
    }
}

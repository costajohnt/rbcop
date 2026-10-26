use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Style/RedundantRegexpConstructor";
pub static COP: RedundantRegexpConstructor = RedundantRegexpConstructor;
pub struct RedundantRegexpConstructor;

impl Cop for RedundantRegexpConstructor {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const {nil? cbase} :Regexp) {:new :compile} $(regexp _* (regopt _*)))
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if !matches!(method, "new" | "compile") || is_csend(&call) {
            return;
        }
        if !call.receiver().is_some_and(|r| is_top_const(&r, "Regexp")) {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        if !matches!(kind::of(arg), kind::REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE) {
            return;
        }
        let (start, end) = send_range(&call);
        let replacement = node_text(arg);
        let ambiguous = ambiguous_slash_regexp(cx.ancestors, start, arg);
        cx.add_fix(start, end, format!("Remove the redundant `Regexp.{method}`."), |c| {
            if !ambiguous {
                c.replace(start, end, replacement)
            }
        });
    }
}

/// `ambiguous_slash_regexp?`: `p / a/` and `p /=a/` parse as division, so a slash regexp starting
/// with whitespace or `=` can't begin the first argument of a command.
fn ambiguous_slash_regexp(ancestors: &[Node], start: usize, regexp: &Node) -> bool {
    // RegexpNode#content: the string parts, joined.
    let (opening, first) = if let Some(r) = regexp.as_regular_expression_node() {
        (r.opening_loc(), r.content_loc().as_slice().first().copied())
    } else {
        let r = regexp.as_interpolated_regular_expression_node().unwrap();
        let first = r
            .parts()
            .iter()
            .find_map(|p| p.as_string_node().and_then(|s| s.content_loc().as_slice().first().copied()));
        (r.opening_loc(), first)
    };
    let ambiguous_start = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b'=');
    if opening.as_slice() != b"/" || !first.is_some_and(ambiguous_start) {
        return false;
    }
    ancestors.iter().any(|a| {
        a.as_call_node().is_some_and(|c| {
            let has_args = c.arguments().is_some() || c.block().is_some_and(|b| b.as_block_argument_node().is_some());
            let command = has_args
                && c.opening_loc().is_none_or(|o| o.as_slice() != b"(")
                && !helpers::node_types::is_operator_method(method_name(&c))
                && c.equal_loc().is_none();
            command && send_args(&c).first().is_some_and(|f| f.location().start_offset() == start)
        })
    })
}

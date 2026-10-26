use crate::helpers::frozen_string_literal as fsl;
use crate::helpers::parser_parent::{is_top_const, parser_parent};
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/EmptyLiteral";
pub static COP: EmptyLiteral = EmptyLiteral;
pub struct EmptyLiteral;

/// `(send (const {nil? cbase} :Name) :meth)` with no arguments (a `&blk` counts as one).
fn const_send(call: &CallNode, name: &str, meth: &str) -> bool {
    method_name(call) == meth
        && call.receiver().is_some_and(|r| is_top_const(&r, name))
        && !is_csend(call)
        && call.block().is_none_or(|b| b.as_block_node().is_some())
}

fn empty_array(node: &Node) -> bool {
    node.as_array_node().is_some_and(|a| a.elements().iter().next().is_none())
}

/// `(send nil? :Name (array))`
fn bare_with_empty_array(call: &CallNode, name: &str) -> bool {
    call.receiver().is_none()
        && method_name(call) == name
        && call.block().is_none()
        && matches!(call_args(call).as_slice(), [a] if empty_array(a))
}

/// `X.new` / `X[]` / `X([])` match, `X.new` only without a block (array_with_block / hash_with_block).
fn offense_node(call: &CallNode, name: &str, allow_array_arg: bool) -> bool {
    let args = call_args(call);
    let new = const_send(call, name, "new")
        && call.block().is_none()
        && (args.is_empty() || (allow_array_arg && matches!(args.as_slice(), [a] if empty_array(a))));
    let index = const_send(call, name, "[]") && args.is_empty() && call.block().is_none();
    new || index || bare_with_empty_array(call, name)
}

impl Cop for EmptyLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "new" | "[]" | "Array" | "Hash") || is_csend(&call) {
            return;
        }
        let (start, end) = send_range(&call);
        let source = cx.src.slice(start, end);
        let hash_new = const_send(&call, "Hash", "new") && call_args(&call).is_empty();
        // array_with_block / hash_with_block on the parser parent: a `X.new { }` block.
        let anc = cx.ancestors;
        let block_parent = |name: &str| {
            parser_parent(anc).is_some_and(|p| p.as_block_node().is_some())
                && anc.iter().rev().skip_while(|a| a.as_block_node().is_none()).nth(1).is_some_and(|o| {
                    o.as_call_node().is_some_and(|o| const_send(&o, name, "new") && call_args(&o).is_empty())
                })
        };
        let mut is_hash = false;
        let (msg, correction) = if offense_node(&call, "Array", true) && !(method_name(&call) == "new" && block_parent("Array")) {
            (format!("Use array literal `[]` instead of `{source}`."), "[]".to_string())
        } else if offense_node(&call, "Hash", false) && !(hash_new && block_parent("Hash")) {
            is_hash = true;
            (format!("Use hash literal `{{}}` instead of `{source}`."), "{}".to_string())
        } else if const_send(&call, "String", "new") && call_args(&call).is_empty() && call.block().is_none() {
            if frozen_strings(cx) {
                return;
            }
            let lit = if cx.config.for_cop("Style/StringLiterals").truthy("Enabled")
                && cx.config.for_cop("Style/StringLiterals").str("EnforcedStyle") == Some("double_quotes")
            {
                "\"\""
            } else {
                "''"
            };
            (format!("Use string literal `{lit}` instead of `String.new`."), lit.to_string())
        } else {
            return;
        };
        // A hash literal as the unparenthesized first argument of a send/super is wrapped in
        // parentheses, the replaced range covering the other arguments.
        let mut range = (start, end);
        let mut correction = correction;
        if is_hash {
            let parent = parser_parent(cx.ancestors);
            let parent_args = parent.and_then(|p| match p.as_call_node() {
                Some(c) if !is_csend(&c) => {
                    Some((send_args(&c), c.closing_loc().is_some_and(|l| l.as_slice() == b")")))
                }
                Some(_) => None,
                None => p.as_super_node().map(|s| {
                    let mut args: Vec<Node> = s.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
                    if let Some(b) = s.block().filter(|b| b.as_block_argument_node().is_some()) {
                        args.push(b);
                    }
                    (args, s.rparen_loc().is_some())
                }),
            });
            if let Some((args, parens)) = parent_args
                && !parens
                && args.first().is_some_and(|a| a.location().start_offset() == start && a.location().end_offset() == end)
            {
                let src = cx.src;
                let rest: Vec<&str> = args[1..]
                    .iter()
                    .map(|a| src.slice(a.location().start_offset(), a.location().end_offset()))
                    .collect();
                range = (start - 1, args.last().unwrap().location().end_offset());
                correction = format!("({})", std::iter::once("{}").chain(rest).collect::<Vec<_>>().join(", "));
            }
        }
        cx.add_fix(start, end, msg, |c| c.replace(range.0, range.1, &correction));
    }
}

/// EmptyLiteral#frozen_strings?
fn frozen_strings(cx: &Cx) -> bool {
    if fsl::enabled(cx) {
        return true;
    }
    cx.config.for_cop("Style/FrozenStringLiteralComment").truthy("Enabled")
        && !fsl::disabled(cx)
        && fsl::frozen_by_default(cx).is_none()
}

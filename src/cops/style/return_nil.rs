use crate::prelude::*;

pub const NAME: &str = "Style/ReturnNil";
pub static COP: ReturnNil = ReturnNil;
pub struct ReturnNil;

impl Cop for ReturnNil {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RETURN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let ret = node.as_return_node().unwrap();
        // `each_ancestor(:block, :any_def)`: stop at a def or lambda; skip out of a non-local
        // return from a block with arguments on a chained send.
        let anc = cx.ancestors;
        for i in (0..anc.len()).rev() {
            if matches!(kind::of(&anc[i]), kind::DEF_NODE | kind::LAMBDA_NODE) {
                break;
            }
            // A call owning a block, reached from its receiver or arguments, is also inside the
            // parser `block` (`foo(proc { return nil }) do |x| ... end`).
            let (block, call) = if let Some(block) = anc[i].as_block_node() {
                (block, i.checked_sub(1).and_then(|j| anc[j].as_call_node()))
            } else if let Some(call) = anc[i].as_call_node()
                && let Some(block) = call.block().and_then(|b| b.as_block_node())
                && anc.get(i + 1).is_none_or(|n| n.as_block_node().is_none())
            {
                (block, Some(call))
            } else {
                continue;
            };
            // numblock / itblock are not `:block`.
            if block.parameters().is_some_and(|p| p.as_block_parameters_node().is_none()) {
                continue;
            }
            if let Some(call) = &call {
                // `lambda?`
                if method_name(call) == "lambda" {
                    break;
                }
                // `(send _ {:define_method :define_singleton_method} _)`
                if !is_csend(call)
                    && matches!(method_name(call), "define_method" | "define_singleton_method")
                    && send_args(call).len() == 1
                {
                    break;
                }
            }
            let has_args = block.parameters().and_then(|p| p.as_block_parameters_node()).is_some_and(|p| {
                p.parameters().is_some_and(|ps| !parameter_nodes(&ps).is_empty()) || p.locals().iter().next().is_some()
            });
            if !has_args {
                continue;
            }
            if call.is_some_and(|c| !is_csend(&c) && c.receiver().is_some()) {
                return;
            }
        }
        let args = ret.arguments().map(|a| a.arguments().iter().collect::<Vec<_>>()).unwrap_or_default();
        let is_return = args.is_empty();
        let is_return_nil = args.len() == 1 && args[0].as_nil_node().is_some();
        let style_return = cx.cfg.str("EnforcedStyle") != Some("return_nil");
        // top_level_return?: no block, lambda or def around it (a call owning a block counts as
        // its parser `block`, also from its receiver or arguments).
        let top_level = !anc.iter().any(|n| {
            matches!(kind::of(n), kind::DEF_NODE | kind::LAMBDA_NODE | kind::BLOCK_NODE)
                || n.as_call_node().is_some_and(|c| c.block().is_some_and(|b| b.as_block_node().is_some()))
        });
        let (ok, msg, corrected) = if style_return {
            (!is_return_nil, "Use `return` instead of `return nil`.", "return")
        } else {
            let parens = helpers::parser_parent::parser_parent(anc)
                .is_some_and(helpers::parser_parent::return_needs_parentheses);
            (
                !is_return || top_level,
                "Use `return nil` instead of `return`.",
                if parens { "(return nil)" } else { "return nil" },
            )
        };
        if ok {
            return;
        }
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        cx.add_fix(start, end, msg, |c| c.replace(start, end, corrected));
    }
}

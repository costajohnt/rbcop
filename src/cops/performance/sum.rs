use crate::prelude::*;

pub const NAME: &str = "Performance/Sum";
pub static COP: Sum = Sum;
pub struct Sum;

/// build_good_method: `sum`, with the initial value unless it is integer zero, and a block pass.
fn good_method(init: Option<&Node>, block_pass: Option<&str>) -> String {
    let mut args: Vec<&str> = Vec::new();
    if let Some(i) = init
        && !(i.as_integer_node().is_some() && int_literal_value(i) == Some(0))
    {
        args.push(node_text(i));
    }
    args.extend(block_pass);
    if args.is_empty() { "sum".to_string() } else { format!("sum({})", args.join(", ")) }
}

fn plus_sym(node: &Node) -> bool {
    node.as_symbol_node().is_some_and(|s| s.unescaped() == b"+")
}

fn plain(b: &ruby_prism::BlockNode) -> bool {
    !b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
}

impl Cop for Sum {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.4, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if !matches!(method, "inject" | "reduce" | "sum") {
            return;
        }
        let safe = cx.cfg.bool("Safe") != Some(false) && cx.cfg.bool("SafeAutoCorrect") != Some(false);
        if let Some(block) = call.block().and_then(|b| b.as_block_node()) {
            self.on_block(cx, &call, &block, safe);
        }
        // empty_array_literal?: `[].inject(:+)` and the like.
        let array_literal = call.receiver().is_some_and(|r| r.as_array_node().is_some());
        if call.receiver().and_then(|r| r.as_array_node()).is_some_and(|a| a.elements().iter().next().is_none()) {
            return;
        }
        let args = send_args(&call);
        // (call _ ${:inject :reduce} $_init ? ${(sym :+) (block_pass (sym :+))})
        if method != "sum" {
            let (init, op) = match args.as_slice() {
                [op] => (None, op),
                [init, op] => (Some(init), op),
                _ => return,
            };
            let block_pass = op.as_block_argument_node().is_some_and(|b| b.expression().is_some_and(|e| plus_sym(&e)));
            if !(plus_sym(op) || block_pass) || init.is_some_and(|i| i.as_block_argument_node().is_some()) {
                return;
            }
            if init.is_none() && cx.cfg.bool("OnlySumOrWithInitialValue") == Some(true) {
                return;
            }
            let good = good_method(init, None);
            let bad = format!(
                "{method}({}{})",
                init.map_or(String::new(), |i| format!("{}, ", node_text(i))),
                if block_pass { "&:+" } else { ":+" }
            );
            let msg = if init.is_none() && !array_literal {
                format!("Use `{good}` instead of `{bad}`, unless calling `{bad}` on an empty array.")
            } else {
                format!("Use `{good}` instead of `{bad}`.")
            };
            let start = call.message_loc().unwrap().start_offset();
            let end = send_range(&call).1;
            let correct = !(init.is_none() && safe);
            cx.add_fix(start, end, msg, |c| {
                if correct {
                    c.replace(start, end, &good);
                }
            });
            return;
        }
        // (call {(block $(call _ {:map :collect}) ...) $(call _ {:map :collect} (block_pass _))} :sum $_init ?)
        if args.len() > 1 || call.block().is_some() {
            // block_literal? / block_argument? (a block pass is an argument too)
            return;
        }
        let init = args.first();
        let Some(map) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if !matches!(method_name(&map), "map" | "collect") || !call_args(&map).is_empty() {
            return;
        }
        let map_block_pass = match map.block() {
            Some(b) if b.as_block_node().is_some() => {
                if !plain(&b.as_block_node().unwrap()) {
                    return;
                }
                None
            }
            Some(b) => Some(node_text(&b)),
            None => return,
        };
        let sum_method = good_method(init, None);
        let dot = map.call_operator_loc().map(|d| text(&d));
        let msg = format!(
            "Use `{sum_method} {{ ... }}` instead of `{} {{ ... }}{}{sum_method}`.",
            method_name(&map),
            dot.unwrap_or(".")
        );
        let start = map.message_loc().unwrap().start_offset();
        let end = send_range(&call).1;
        // method_call_with_args_range: from the receiver's end (or the whole call).
        let sum_start = node.as_call_node().unwrap().receiver().unwrap().location().end_offset();
        let map_end = send_range(&map).1;
        let map_start = map.receiver().map_or(map.location().start_offset(), |r| r.location().end_offset());
        let replacement = format!("{}{}", dot.unwrap_or(""), good_method(init, map_block_pass));
        cx.add_fix(start, end, msg, |c| {
            c.remove(sum_start, end);
            c.replace(map_start, map_end, &replacement);
        });
    }
}

impl Sum {
    /// on_block: `inject(init) { |acc, elem| acc + elem }`.
    fn on_block(&self, cx: &mut Cx, call: &ruby_prism::CallNode, block: &ruby_prism::BlockNode, safe: bool) {
        let method = method_name(call);
        if method == "sum" {
            return;
        }
        let args = send_args(call);
        if args.len() > 1 {
            return;
        }
        let init = args.first();
        let Some(bp) = block.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
        if bp.locals().iter().count() != 0 {
            return;
        }
        let Some(ps) = bp.parameters() else { return };
        let params = parameter_nodes(&ps);
        let [acc, elem] = params.as_slice() else { return };
        let (Some(acc), Some(elem)) = (acc.as_required_parameter_node(), elem.as_required_parameter_node()) else {
            return;
        };
        let (acc, elem) = (acc.name().as_slice(), elem.name().as_slice());
        let Some(stmts) = block.body().and_then(|b| b.as_statements_node()) else { return };
        let body: Vec<Node> = stmts.body().iter().collect();
        let [body] = body.as_slice() else { return };
        let Some(plus) = body.as_call_node() else { return };
        if is_csend(&plus) || method_name(&plus) != "+" || plus.block().is_some() {
            return;
        }
        let plus_args = call_args(&plus);
        let [rhs] = plus_args.as_slice() else { return };
        let Some(lhs) = plus.receiver() else { return };
        let lvar = |n: &Node, name: &[u8]| n.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == name);
        if !((lvar(&lhs, acc) && lvar(rhs, elem)) || (lvar(&lhs, elem) && lvar(rhs, acc))) {
            return;
        }
        let good = good_method(init, None);
        let bad = format!(
            "{method}{} {{ |{}, {}| {} }}",
            init.map_or(String::new(), |i| format!("({})", node_text(i))),
            String::from_utf8_lossy(acc),
            String::from_utf8_lossy(elem),
            node_text(body)
        );
        let start = call.message_loc().unwrap().start_offset();
        let end = block.closing_loc().end_offset();
        let correct = !(init.is_none() && safe);
        cx.add_fix(start, end, format!("Use `{good}` instead of `{bad}`."), |c| {
            if correct {
                c.replace(start, end, &good);
            }
        });
    }
}

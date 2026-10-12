use crate::prelude::*;

pub const NAME: &str = "Performance/CompareWithBlock";
pub static COP: CompareWithBlock = CompareWithBlock;
pub struct CompareWithBlock;

impl Cop for CompareWithBlock {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block $(send _ {:sort :sort! :min :max :minmax}) (args (arg $_a) (arg $_b)) $send)
        let call = node.as_call_node().unwrap();
        let compare = method_name(&call);
        let replacement_method = match compare {
            "sort" => "sort_by",
            "sort!" => "sort_by!",
            "min" => "min_by",
            "max" => "max_by",
            "minmax" => "minmax_by",
            _ => return,
        };
        if is_csend(&call) || !call_args(&call).is_empty() {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let Some(bp) = block.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
        if bp.locals().iter().count() != 0 {
            return;
        }
        let Some(ps) = bp.parameters() else { return };
        let params = parameter_nodes(&ps);
        let [a, b] = params.as_slice() else { return };
        let (Some(a), Some(b)) = (a.as_required_parameter_node(), b.as_required_parameter_node()) else { return };
        let (var_a, var_b) = (a.name().as_slice(), b.name().as_slice());
        let Some(stmts) = block.body().and_then(|b| b.as_statements_node()) else { return };
        let body: Vec<Node> = stmts.body().iter().collect();
        let [cmp] = body.as_slice() else { return };
        // (send (send (lvar %1) $_method $...) :<=> (send (lvar %2) _method $...))
        let Some(cmp) = cmp.as_call_node() else { return };
        if is_csend(&cmp) || method_name(&cmp) != "<=>" || cmp.block().is_some() {
            return;
        }
        let cmp_args = call_args(&cmp);
        let [rhs] = cmp_args.as_slice() else { return };
        let (Some(lhs), Some(rhs)) = (cmp.receiver().and_then(|r| r.as_call_node()), rhs.as_call_node()) else {
            return;
        };
        let on = |c: &ruby_prism::CallNode, name: &[u8]| {
            !is_csend(c)
                && !c.block().is_some_and(|b| b.as_block_node().is_some())
                && c.receiver().and_then(|r| r.as_local_variable_read_node()).is_some_and(|l| l.name().as_slice() == name)
        };
        let method = method_name(&lhs);
        if !on(&lhs, var_a) || !on(&rhs, var_b) || method_name(&rhs) != method {
            return;
        }
        // slow_compare?: equal argument lists; `[]` with one sym/str/int key, else none.
        let (args_a, args_b) = (send_args(&lhs), send_args(&rhs));
        let span = |n: &Node| (n.location().start_offset(), n.location().end_offset());
        if args_a.len() != args_b.len() || args_a.iter().zip(&args_b).any(|(x, y)| !helpers::node_equality::same(cx.src, span(x), span(y))) {
            return;
        }
        let (var_a, var_b) = (String::from_utf8_lossy(var_a), String::from_utf8_lossy(var_b));
        let (instead, str_a, str_b) = if method == "[]" {
            let [key] = args_a.as_slice() else { return };
            let key_ok = key.as_symbol_node().is_some_and(|_| !is_parser_dsym(key, cx.config.target_ruby))
                || (key.as_string_node().is_some() && !is_parser_dstr(key))
                || key.as_integer_node().is_some();
            if !key_ok {
                return;
            }
            let k = node_text(key);
            (format!(" {{ |a| a[{k}] }}"), format!("{var_a}[{k}]"), format!("{var_b}[{k}]"))
        } else {
            if !args_a.is_empty() {
                return;
            }
            (format!("(&:{method})"), format!("{var_a}.{method}"), format!("{var_b}.{method}"))
        };
        let msg = format!(
            "Use `{replacement_method}{instead}` instead of `{compare} {{ |{var_a}, {var_b}| {str_a} <=> {str_b} }}`."
        );
        let start = call.message_loc().unwrap().start_offset();
        let end = block.closing_loc().end_offset();
        let replacement = format!("{replacement_method}{instead}");
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}

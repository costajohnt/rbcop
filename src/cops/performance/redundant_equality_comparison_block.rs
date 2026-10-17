use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Performance/RedundantEqualityComparisonBlock";
pub static COP: RedundantEqualityComparisonBlock = RedundantEqualityComparisonBlock;
pub struct RedundantEqualityComparisonBlock;

const TARGET_METHODS: &[&str] = &["all?", "any?", "one?", "none?"];
const COMPARISON_METHODS: &[&str] = &["==", "===", "is_a?", "kind_of?"];
const REGEXP_METHODS: &[&str] = &["=~", "match?"];

impl Cop for RedundantEqualityComparisonBlock {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.5, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !TARGET_METHODS.contains(&method_name(&call)) {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        // one_block_argument?: one parameter, and no comma in the parameter list.
        let Some(params) = block.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
        let Some(list) = params.parameters() else { return };
        let mut nodes = parameter_nodes(&list);
        nodes.retain(|n| n.as_implicit_rest_node().is_none());
        if nodes.len() != 1 || params.locals().iter().count() != 0 {
            return;
        }
        let args_src = &cx.src.text[list.location().start_offset()..list.location().end_offset()];
        if args_src.contains(',') {
            return;
        }
        let arg = node_text(&nodes[0]);
        // The body: a single `send`.
        let Some(stmts) = block.body().and_then(|b| b.as_statements_node()) else { return };
        let body: Vec<Node> = stmts.body().iter().collect();
        let [body] = body.as_slice() else { return };
        let Some(cmp) = body.as_call_node() else { return };
        if is_csend(&cmp) || cmp.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let name = method_name(&cmp);
        let allow_regexp = !matches!(cx.cfg.get("AllowRegexpMatch"), Some(Value::Null | Value::Bool(false)));
        if !COMPARISON_METHODS.contains(&name) && (allow_regexp || !REGEXP_METHODS.contains(&name)) {
            return;
        }
        let cmp_args = send_args(&cmp);
        // `first_argument.source` / `receiver.source` on nil raise: no offense.
        let Some(first) = cmp_args.first() else { return };
        let receiver = cmp.receiver();
        // same_block_argument_and_is_a_argument?
        let skip = if name == "===" {
            arg != node_text(first)
        } else if name == "is_a?" || name == "kind_of?" {
            arg == node_text(first)
        } else {
            let Some(r) = &receiver else { return };
            Some(node_text(r)) == receiver_source(first)
        };
        if skip {
            return;
        }
        let Some(r) = receiver else { return };
        let new_argument = if arg == node_text(&r) {
            if uses_argument(arg, first) {
                return;
            }
            node_text(first)
        } else if arg == node_text(first) {
            if uses_argument(arg, &r) {
                return;
            }
            node_text(&r)
        } else {
            return;
        };
        let prefer = format!("{}({new_argument})", method_name(&call));
        let start = call.message_loc().unwrap().start_offset();
        let end = node.location().end_offset();
        cx.add_fix(start, end, format!("Use `{prefer}` instead of block."), |c| c.replace(start, end, &prefer));
    }
}

/// rubocop-ast `receiver` (send, csend or block) source, nil for other nodes.
fn receiver_source<'pr>(node: &Node<'pr>) -> Option<&'pr str> {
    node.as_call_node()?.receiver().map(|r| node_text(&r))
}

/// use_block_argument_in_method_argument_of_operand?: a `send` operand whose arguments (or the
/// local variables in them) are spelled as the block argument.
fn uses_argument(arg: &str, operand: &Node) -> bool {
    let Some(call) = operand.as_call_node() else { return false };
    if is_csend(&call) || call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return false;
    }
    let args = send_args(&call);
    let mut lvars = Lvars(Vec::new());
    for a in &args {
        lvars.visit(a);
    }
    args.iter().any(|a| node_text(a) == arg) || lvars.0.iter().any(|l| l == arg)
}

/// Sources of `lvar` descendants.
struct Lvars(Vec<String>);

impl<'pr> Visit<'pr> for Lvars {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        self.0.push(String::from_utf8_lossy(node.location().as_slice()).into_owned());
    }
}

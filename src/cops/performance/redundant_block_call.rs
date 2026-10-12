use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Performance/RedundantBlockCall";
pub static COP: RedundantBlockCall = RedundantBlockCall;
pub struct RedundantBlockCall;

impl Cop for RedundantBlockCall {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        // (def _ (args ... (blockarg $_)) $_), with a named block argument and a body.
        let Some(name) = def.parameters().and_then(|p| p.block()).and_then(|b| b.name()) else { return };
        let Some(body) = def.body() else { return };
        let name = name.as_slice();
        // blockarg_assigned? / shadowed_block_argument?
        let mut search = Search { name, assigned: false, calls: Vec::new() };
        search.visit(&body);
        if search.assigned || shadowed(&body, name) {
            return;
        }
        // Any call passing a block (`blk.call(&other)`) turns every report off.
        if search.calls.iter().any(|c| c.block_pass) {
            return;
        }
        let argname = String::from_utf8_lossy(name).into_owned();
        for call in &search.calls {
            // block_literal?: `blk.call { }`
            if call.literal_block {
                continue;
            }
            let mut new_source = String::from("yield");
            if !call.args.is_empty() {
                new_source.push_str(if call.paren { "(" } else { " " });
                new_source.push_str(&call.args.join(", "));
                if call.paren {
                    new_source.push(')');
                }
            }
            let (start, end) = call.range;
            cx.add_fix(start, end, format!("Use `yield` instead of `{argname}.call`."), |c| {
                c.replace(start, end, &new_source)
            });
        }
    }
}

/// shadowed_block_argument?: the body is a single `block` one of whose parameters is spelled as
/// the block argument.
fn shadowed(body: &Node, name: &[u8]) -> bool {
    let Some(stmts) = body.as_statements_node() else { return false };
    let list: Vec<Node> = stmts.body().iter().collect();
    let [only] = list.as_slice() else { return false };
    let Some(block) = only.as_call_node().and_then(|c| c.block()).and_then(|b| b.as_block_node()) else {
        return false;
    };
    let Some(params) = block.parameters().and_then(|p| p.as_block_parameters_node()) else { return false };
    let mut sources: Vec<&[u8]> = params.locals().iter().map(|l| l.location().as_slice()).collect();
    if let Some(ps) = params.parameters() {
        let ps_nodes = parameter_nodes(&ps);
        sources.extend(ps_nodes.iter().filter(|n| n.as_implicit_rest_node().is_none()).map(|n| n.location().as_slice()));
    }
    sources.contains(&name)
}

struct Search<'n, 'pr> {
    name: &'n [u8],
    assigned: bool,
    calls: Vec<BlockCall<'pr>>,
}

/// A `blk.call(...)`.
struct BlockCall<'pr> {
    range: (usize, usize),
    args: Vec<&'pr str>,
    paren: bool,
    literal_block: bool,
    block_pass: bool,
}

impl<'pr> Visit<'pr> for Search<'_, 'pr> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        // (send (lvar %1) :call ...)
        if !is_csend(node)
            && method_name(node) == "call"
            && node.receiver().and_then(|r| r.as_local_variable_read_node()).is_some_and(|l| l.name().as_slice() == self.name)
        {
            self.calls.push(BlockCall {
                range: send_range(node),
                args: call_args(node).iter().map(|a| node_text(a)).collect(),
                paren: node.closing_loc().is_some_and(|c| c.as_slice() == b")"),
                literal_block: node.block().is_some_and(|b| b.as_block_node().is_some()),
                block_pass: node.block().is_some_and(|b| b.as_block_argument_node().is_some()),
            });
        }
        ruby_prism::visit_call_node(self, node);
    }
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.assigned |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.assigned |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.assigned |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.assigned |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        self.assigned |= node.name().as_slice() == self.name;
    }
    // Pattern variables are `match_var` and named captures `match_with_lvasgn`: no lvasgn.
    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        if let Some(s) = node.statements() {
            self.visit_statements_node(&s);
        }
    }
    fn visit_match_predicate_node(&mut self, node: &ruby_prism::MatchPredicateNode<'pr>) {
        self.visit(&node.value());
    }
    fn visit_match_required_node(&mut self, node: &ruby_prism::MatchRequiredNode<'pr>) {
        self.visit(&node.value());
    }
    fn visit_match_write_node(&mut self, node: &ruby_prism::MatchWriteNode<'pr>) {
        self.visit_call_node(&node.call());
    }
}

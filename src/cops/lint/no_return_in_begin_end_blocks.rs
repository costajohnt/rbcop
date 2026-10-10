use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Lint/NoReturnInBeginEndBlocks";
pub static COP: NoReturnInBeginEndBlocks = NoReturnInBeginEndBlocks;
pub struct NoReturnInBeginEndBlocks;

/// Walks an assigned value: `true` frames are `begin..end` (kwbegin), `false` frames are scopes a
/// `return` leaves (def, lambda). A return counts when its innermost frame is a kwbegin.
#[derive(Default)]
struct Returns {
    stack: Vec<bool>,
    found: Vec<(usize, usize)>,
}

impl<'pr> Visit<'pr> for Returns {
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'pr>) {
        let kw = node.begin_keyword_loc().is_some();
        if kw {
            self.stack.push(true);
        }
        ruby_prism::visit_begin_node(self, node);
        if kw {
            self.stack.pop();
        }
    }
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        self.stack.push(false);
        ruby_prism::visit_def_node(self, node);
        self.stack.pop();
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        self.stack.push(false);
        ruby_prism::visit_lambda_node(self, node);
        self.stack.pop();
    }
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        // `lambda { }`: parser's block (incl. its send) is the scope.
        let lambda = method_name(node) == "lambda" && node.block().is_some_and(|b| b.as_block_node().is_some());
        if lambda {
            self.stack.push(false);
        }
        ruby_prism::visit_call_node(self, node);
        if lambda {
            self.stack.pop();
        }
    }
    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode<'pr>) {
        if self.stack.last() == Some(&true) {
            let l = node.location();
            self.found.push((l.start_offset(), l.end_offset()));
        }
        ruby_prism::visit_return_node(self, node);
    }
}

/// The assigned value of lvasgn ivasgn cvasgn gvasgn casgn or_asgn op_asgn (not and_asgn).
fn value<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    macro_rules! try_value {
        ($($m:ident),*) => {$(
            if let Some(n) = node.$m() {
                return Some(n.value());
            }
        )*};
    }
    try_value!(
        as_local_variable_write_node,
        as_instance_variable_write_node,
        as_class_variable_write_node,
        as_global_variable_write_node,
        as_constant_write_node,
        as_constant_path_write_node,
        as_local_variable_or_write_node,
        as_instance_variable_or_write_node,
        as_class_variable_or_write_node,
        as_global_variable_or_write_node,
        as_constant_or_write_node,
        as_constant_path_or_write_node,
        as_call_or_write_node,
        as_index_or_write_node,
        as_local_variable_operator_write_node,
        as_instance_variable_operator_write_node,
        as_class_variable_operator_write_node,
        as_global_variable_operator_write_node,
        as_constant_operator_write_node,
        as_constant_path_operator_write_node,
        as_call_operator_write_node,
        as_index_operator_write_node
    );
    None
}

impl Cop for NoReturnInBeginEndBlocks {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::LOCAL_VARIABLE_WRITE_NODE,
            kind::INSTANCE_VARIABLE_WRITE_NODE,
            kind::CLASS_VARIABLE_WRITE_NODE,
            kind::GLOBAL_VARIABLE_WRITE_NODE,
            kind::CONSTANT_WRITE_NODE,
            kind::CONSTANT_PATH_WRITE_NODE,
            kind::LOCAL_VARIABLE_OR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
            kind::CLASS_VARIABLE_OR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
            kind::CONSTANT_OR_WRITE_NODE,
            kind::CONSTANT_PATH_OR_WRITE_NODE,
            kind::CALL_OR_WRITE_NODE,
            kind::INDEX_OR_WRITE_NODE,
            kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
            kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::CONSTANT_OPERATOR_WRITE_NODE,
            kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
            kind::CALL_OPERATOR_WRITE_NODE,
            kind::INDEX_OPERATOR_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(value) = value(node) else { return };
        // ponytail: index/call or-asgn receivers and arguments are not searched, only the value.
        let mut v = Returns::default();
        v.visit(&value);
        for (s, e) in v.found {
            cx.add(s, e, "Do not `return` in `begin..end` blocks in assignment contexts.");
        }
    }
}

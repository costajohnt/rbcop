use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Performance/BlockGivenWithExplicitBlock";
pub static COP: BlockGivenWithExplicitBlock = BlockGivenWithExplicitBlock;
pub struct BlockGivenWithExplicitBlock;

impl Cop for BlockGivenWithExplicitBlock {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "block_given?" {
            return;
        }
        // each_ancestor(:any_def).first
        let Some(def) = cx.ancestors.iter().rev().find_map(|a| a.as_def_node()) else { return };
        let Some(block_arg) = def.parameters().and_then(|p| p.block()) else { return };
        let Some(name) = block_arg.name() else { return };
        let name = String::from_utf8_lossy(name.as_slice()).into_owned();
        // reassigns_block_arg?: `(lvasgn %1 ...)` anywhere in the def.
        let mut finder = Assigns { name: name.as_bytes(), found: false };
        finder.visit(&def.as_node());
        if finder.found {
            return;
        }
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, "Check block argument explicitly instead of using `block_given?`.", |c| {
            c.replace(start, end, &name)
        });
    }
}

/// Parser `lvasgn` nodes of one name: writes, op-assigns, multiple-assignment, rescue and `for`
/// targets (not pattern variables, which are `match_var`).
struct Assigns<'a> {
    name: &'a [u8],
    found: bool,
}

impl<'pr> Visit<'pr> for Assigns<'_> {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
    }
    // Pattern variables are `match_var`: visit only the matched values.
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
    // `/(?<b>.)/ =~ s` is `match_with_lvasgn`, which has no lvasgn children.
    fn visit_match_write_node(&mut self, node: &ruby_prism::MatchWriteNode<'pr>) {
        self.visit_call_node(&node.call());
    }
}

use crate::helpers::gemspec_help::is_spec_new;
use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Gemspec/DeprecatedAttributeAssignment";
pub static COP: DeprecatedAttributeAssignment = DeprecatedAttributeAssignment;
pub struct DeprecatedAttributeAssignment;

const ATTRIBUTES: [&str; 4] = ["test_files", "date", "specification_version", "rubygems_version"];

/// `block_node.descendants.detect { use_deprecated_attributes? }`: first match in pre-order.
struct Find<'a> {
    param: &'a str,
    found: Option<(usize, usize, &'static str)>,
}

impl Find<'_> {
    fn receiver_ok(&self, recv: Option<Node>) -> bool {
        recv.is_some_and(|r| node_text(&r) == self.param)
    }
    fn setter(&mut self, name: &[u8], recv: Option<Node>, range: (usize, usize)) {
        if self.found.is_some() || !self.receiver_ok(recv) {
            return;
        }
        if let Some(a) = ATTRIBUTES.iter().find(|a| name.strip_suffix(b"=") == Some(a.as_bytes())) {
            self.found = Some((range.0, range.1, a));
        }
    }
}

impl<'pr> Visit<'pr> for Find<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        if !is_csend(node) {
            self.setter(node.name().as_slice(), node.receiver(), send_range(node));
        }
        ruby_prism::visit_call_node(self, node);
    }
    fn visit_call_target_node(&mut self, node: &ruby_prism::CallTargetNode<'pr>) {
        if !node.is_safe_navigation() {
            let l = node.location();
            self.setter(node.name().as_slice(), Some(node.receiver()), (l.start_offset(), l.end_offset()));
        }
        ruby_prism::visit_call_target_node(self, node);
    }
    fn visit_call_operator_write_node(&mut self, node: &ruby_prism::CallOperatorWriteNode<'pr>) {
        // `node_and_method_name` reassigns `node` to the op_asgn's lhs on the first attribute, so
        // only `test_files` is ever matched on an op_asgn (by its reader name).
        if self.found.is_none()
            && !node.is_safe_navigation()
            && node.read_name().as_slice() == b"test_files"
            && self.receiver_ok(node.receiver())
        {
            let l = node.location();
            self.found = Some((l.start_offset(), l.end_offset(), "test_files"));
        }
        ruby_prism::visit_call_operator_write_node(self, node);
    }
}

impl Cop for DeprecatedAttributeAssignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !is_spec_new(&call) {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        // `block_node.first_argument.source`; numblock/itblock never reach `on_block`.
        let Some(params) = block.parameters() else { return };
        let Some(params) = params.as_block_parameters_node() else { return };
        let first = params
            .parameters()
            .and_then(|p| parameter_nodes(&p).into_iter().next())
            .or_else(|| params.locals().iter().next());
        let Some(first) = first else { return };
        let mut find = Find { param: node_text(&first), found: None };
        find.visit_block_node(&block);
        let Some((s, e, attribute)) = find.found else { return };
        let (rs, re) = helpers::whole_lines::range_by_whole_lines(cx.src.text, s, e, true);
        cx.add_fix(s, e, format!("Do not set `{attribute}` in gemspec."), |c| c.remove(rs, re));
    }
}

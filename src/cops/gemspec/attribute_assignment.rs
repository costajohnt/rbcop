use crate::helpers::gemspec_help::{lvar_name, spec_block_var};
use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Gemspec/AttributeAssignment";
pub static COP: AttributeAssignment = AttributeAssignment;
pub struct AttributeAssignment;

/// AST::Node#assignment_method?
fn assignment_method(name: &[u8]) -> bool {
    name.ends_with(b"=") && !matches!(name, b"==" | b"===" | b"!=" | b"<=" | b">=")
}

type Indexed<'pr> = (&'pr [u8], &'pr [u8], (usize, usize));

#[derive(Default)]
struct Collect<'pr> {
    target_ruby: f64,
    /// Block variable of the first `Gem::Specification.new do |x|`.
    spec_var: Option<&'pr [u8]>,
    /// (receiver, attribute) of `x.attr = v`
    plain: Vec<(&'pr [u8], Vec<u8>)>,
    /// (receiver, attribute, range) of `x.attr[lit] = v`
    indexed: Vec<Indexed<'pr>>,
}

impl<'pr> Collect<'pr> {
    fn plain_send(&mut self, recv: &Node<'pr>, name: &'pr [u8]) {
        // (send (lvar ...) _ ...) .select(&:assignment_method?)
        if let Some(r) = lvar_name(recv, self.target_ruby)
            && assignment_method(name)
        {
            self.plain.push((r, name[..name.len() - 1].to_vec()));
        }
    }
}

impl<'pr> Visit<'pr> for Collect<'pr> {
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        if self.spec_var.is_none() {
            self.spec_var = spec_block_var(node);
        }
        if !is_csend(node)
            && let Some(recv) = node.receiver()
        {
            self.plain_send(&recv, node.name().as_slice());
            // (send (send (lvar ...) _) :[]= literal? _)
            let args = send_args(node);
            if method_name(node) == "[]="
                && args.len() == 2
                && helpers::literal_node::literal(&args[0])
                && let Some(inner) = recv.as_call_node()
                && !is_csend(&inner)
                && inner.block().is_none()
                && send_args(&inner).is_empty()
                && let Some(r) = inner.receiver().and_then(|r| lvar_name(&r, self.target_ruby))
            {
                self.indexed.push((r, inner.name().as_slice(), send_range(node)));
            }
        }
        ruby_prism::visit_call_node(self, node);
    }
    fn visit_call_target_node(&mut self, node: &ruby_prism::CallTargetNode<'pr>) {
        if !node.is_safe_navigation() {
            self.plain_send(&node.receiver(), node.name().as_slice());
        }
        ruby_prism::visit_call_target_node(self, node);
    }
}

impl Cop for AttributeAssignment {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let mut c = Collect { target_ruby: cx.config.target_ruby, ..Collect::default() };
        c.visit(node);
        // match_block_variable_name?: compares with the first specification's variable (nil
        // without one: `def_node_search` with a block returns nil).
        let spec_var = c.spec_var;
        let matches = |r: &[u8]| matches!(r, b"_1" | b"it") || spec_var == Some(r);
        let attrs: Vec<&[u8]> = c.plain.iter().filter(|(r, _)| matches(r)).map(|(_, a)| a.as_slice()).collect();
        for (r, attr, (s, e)) in &c.indexed {
            if matches(r) && attrs.contains(attr) {
                cx.add(*s, *e, "Use consistent style for Gemspec attributes assignment.");
            }
        }
    }
}

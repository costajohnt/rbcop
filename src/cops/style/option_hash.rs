use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/OptionHash";
pub static COP: OptionHash = OptionHash;
pub struct OptionHash;

#[derive(Default)]
struct SuperFinder(bool);

impl<'pr> Visit<'pr> for SuperFinder {
    fn visit_super_node(&mut self, _: &ruby_prism::SuperNode<'pr>) {
        self.0 = true;
    }
    fn visit_forwarding_super_node(&mut self, _: &ruby_prism::ForwardingSuperNode<'pr>) {
        self.0 = true;
    }
}

impl Cop for OptionHash {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::BLOCK_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // the args' parent (def or block, the block including its send) and its method name
        let (params, parent_name, shadow) = match node {
            Node::DefNode { .. } => {
                let d = node.as_def_node().unwrap();
                (d.parameters(), String::from_utf8_lossy(d.name().as_slice()).into_owned(), false)
            }
            _ => {
                let bp = match node {
                    Node::BlockNode { .. } => node.as_block_node().unwrap().parameters(),
                    _ => node.as_lambda_node().unwrap().parameters(),
                };
                let Some(bp) = bp.and_then(|p| p.as_block_parameters_node()) else { return };
                let name = if node.as_lambda_node().is_some() {
                    "lambda".to_string()
                } else {
                    // a `super do |opts = {}| end` block's send is a super: super_used?
                    let Some(owner) = cx.ancestors.last().and_then(|a| a.as_call_node()) else { return };
                    method_name(&owner).to_string()
                };
                (bp.parameters(), name, bp.locals().iter().next().is_some())
            }
        };
        let Some(params) = params else { return };
        // (args ... $(optarg [#suspicious_name? _] (hash)))
        if shadow {
            return;
        }
        let Some(last) = parameter_nodes(&params).pop() else { return };
        let Some(opt) = last.as_optional_parameter_node() else { return };
        if opt.value().as_hash_node().is_none_or(|h| h.elements().iter().next().is_some()) {
            return;
        }
        let name = String::from_utf8_lossy(opt.name().as_slice()).into_owned();
        if cx.cfg.get("SuspiciousParamNames").is_none() || !cx.cfg.strings("SuspiciousParamNames").contains(&name) {
            return;
        }
        if cx.cfg.strings("Allowlist").contains(&parent_name) {
            return;
        }
        // super_used?: node.parent.each_node(:zsuper, :super)
        let mut finder = SuperFinder::default();
        match node {
            Node::BlockNode { .. } => finder.visit(cx.ancestors.last().unwrap()),
            _ => finder.visit(node),
        }
        if finder.0 {
            return;
        }
        let loc = last.location();
        cx.add(loc.start_offset(), loc.end_offset(), "Prefer keyword arguments to options hashes.");
    }
}

use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Naming/BinaryOperatorParameterName";
pub static COP: BinaryOperatorParameterName = BinaryOperatorParameterName;
pub struct BinaryOperatorParameterName;

const OP_LIKE_METHODS: &[&str] = &["eql?", "equal?"];
const EXCLUDED: &[&str] = &["+@", "-@", "[]", "[]=", "<<", "===", "`", "=~"];

fn op_method(name: &str) -> bool {
    if EXCLUDED.contains(&name) {
        return false;
    }
    // !/\A[[:word:]]/
    !name.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_') || OP_LIKE_METHODS.contains(&name)
}

impl Cop for BinaryOperatorParameterName {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        if def.receiver().is_some() {
            return;
        }
        let name = std::str::from_utf8(def.name().as_slice()).unwrap_or("");
        if !op_method(name) {
            return;
        }
        let Some(params) = def.parameters() else { return };
        if params.requireds().len() != 1
            || !params.optionals().is_empty()
            || crate::ast::parser_rest(params.rest()).is_some()
            || !params.posts().is_empty()
            || !params.keywords().is_empty()
            || params.keyword_rest().is_some()
            || params.block().is_some()
        {
            return;
        }
        let Some(arg) = params.requireds().iter().next().and_then(|a| a.as_required_parameter_node()) else {
            return;
        };
        let arg_name = arg.name().as_slice();
        if arg_name == b"other" || arg_name == b"_other" {
            return;
        }
        let loc = arg.location();
        let mut refs = Refs { name: arg_name, shadow: 0, found: Vec::new() };
        if let Some(body) = def.body() {
            refs.visit(&body);
        }
        let found = refs.found;
        let msg = format!("When defining the `{name}` operator, name its argument `other`.");
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| {
            c.replace(loc.start_offset(), loc.end_offset(), "other");
            for (s, e) in found {
                c.replace(s, e, "other");
            }
        });
    }
}

/// `lvar`/`lvasgn` name locations matching the parameter, outside blocks that redeclare it.
struct Refs<'a> {
    name: &'a [u8],
    shadow: usize,
    found: Vec<(usize, usize)>,
}

impl Refs<'_> {
    fn hit(&mut self, name: &[u8], loc: ruby_prism::Location) {
        if self.shadow == 0 && name == self.name {
            self.found.push((loc.start_offset(), loc.end_offset()));
        }
    }

    fn redeclares(&self, params: Option<Node>) -> bool {
        let Some(bp) = params.and_then(|p| p.as_block_parameters_node()) else { return false };
        let mut names: Vec<Vec<u8>> = bp.locals().iter().filter_map(|l| param_name(&l)).collect();
        if let Some(p) = bp.parameters() {
            names.extend(parameter_nodes(&p).iter().filter_map(param_name));
        }
        names.iter().any(|n| n == self.name)
    }
}

fn param_name(node: &Node) -> Option<Vec<u8>> {
    let id = match node {
        Node::RequiredParameterNode { .. } => node.as_required_parameter_node()?.name(),
        Node::OptionalParameterNode { .. } => node.as_optional_parameter_node()?.name(),
        Node::RestParameterNode { .. } => node.as_rest_parameter_node()?.name()?,
        Node::RequiredKeywordParameterNode { .. } => node.as_required_keyword_parameter_node()?.name(),
        Node::OptionalKeywordParameterNode { .. } => node.as_optional_keyword_parameter_node()?.name(),
        Node::KeywordRestParameterNode { .. } => node.as_keyword_rest_parameter_node()?.name()?,
        Node::BlockParameterNode { .. } => node.as_block_parameter_node()?.name()?,
        Node::BlockLocalVariableNode { .. } => node.as_block_local_variable_node()?.name(),
        _ => return None,
    };
    Some(id.as_slice().to_vec())
}

impl<'pr> Visit<'pr> for Refs<'_> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        self.hit(node.name().as_slice(), node.location());
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        self.hit(node.name().as_slice(), node.location());
    }
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.hit(node.name().as_slice(), node.name_loc());
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.hit(node.name().as_slice(), node.name_loc());
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.hit(node.name().as_slice(), node.name_loc());
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.hit(node.name().as_slice(), node.name_loc());
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        let shadows = self.redeclares(node.parameters());
        self.shadow += shadows as usize;
        ruby_prism::visit_block_node(self, node);
        self.shadow -= shadows as usize;
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        let shadows = self.redeclares(node.parameters());
        self.shadow += shadows as usize;
        ruby_prism::visit_lambda_node(self, node);
        self.shadow -= shadows as usize;
    }
}

use crate::prelude::*;
use ruby_prism::{ItLocalVariableReadNode, LocalVariableReadNode, Visit};

pub const NAME: &str = "Style/ItBlockParameter";
pub static COP: ItBlockParameter = ItBlockParameter;
pub struct ItBlockParameter;

const MSG_USE: &str = "Use `it` block parameter.";
const MSG_AVOID: &str = "Avoid using `it` block parameter.";
const MSG_AVOID_MULTILINE: &str = "Avoid using `it` block parameter for multi-line blocks.";

/// `lvar` nodes (by source) in a subtree.
struct Lvars<'a> {
    name: &'a str,
    out: Vec<(usize, usize)>,
}

impl<'pr> Visit<'pr> for Lvars<'_> {
    fn visit_local_variable_read_node(&mut self, node: &LocalVariableReadNode<'pr>) {
        // `d.source == name`: parser's lvar is the bare name even for `foo(name:)`.
        if node.name().as_slice() == self.name.as_bytes() {
            self.out.push(helpers::lvar::range(&node.location(), self.name.as_bytes()));
        }
    }
    fn visit_it_local_variable_read_node(&mut self, node: &ItLocalVariableReadNode<'pr>) {
        if self.name == "it" {
            self.out.push(helpers::lvar::range(&node.location(), b"it"));
        }
    }
}

/// The parser body when it is a single statement (the body node itself otherwise).
fn single_stmt<'pr>(body: &Node<'pr>) -> Option<Node<'pr>> {
    body.as_statements_node().and_then(|s| {
        let mut it = s.body().iter();
        let first = it.next();
        if it.next().is_some() { None } else { first }
    })
}

/// block_variables: `body.each_descendant(:lvar).select { |d| d.source == name }` (the parser
/// body itself excluded).
fn block_variables(body: &Node, name: &str) -> Vec<(usize, usize)> {
    let single = single_stmt(body);
    let body = single.as_ref().unwrap_or(body);
    let mut v = Lvars { name, out: Vec::new() };
    v.visit(body);
    let l = body.location();
    v.out.retain(|&r| r != (l.start_offset(), l.end_offset()));
    v.out
}

/// inner_block_ranges: `{`..`}` / `do`..`end` of every block in the body (itself included).
fn inner_block_ranges(body: &Node) -> Vec<(usize, usize)> {
    #[derive(Default)]
    struct Blocks(Vec<(usize, usize)>);
    impl<'pr> Visit<'pr> for Blocks {
        fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
            self.0.push((node.opening_loc().start_offset(), node.closing_loc().end_offset()));
            ruby_prism::visit_block_node(self, node);
        }
        fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
            self.0.push((node.opening_loc().start_offset(), node.closing_loc().end_offset()));
            ruby_prism::visit_lambda_node(self, node);
        }
    }
    let mut v = Blocks::default();
    v.visit(single_stmt(body).as_ref().unwrap_or(body));
    v.0
}

fn inside(ranges: &[(usize, usize)], r: (usize, usize)) -> bool {
    ranges.iter().any(|&(a, b)| a <= r.0 && r.1 <= b)
}

/// variable_definitions: parser `lvasgn`, `match_var` and `match_with_lvasgn` names, with the
/// definition's range and the start of its nearest def/class/module/sclass scope.
/// (name, range, start of the nearest scope)
type Definition = (Vec<u8>, (usize, usize), Option<usize>);
/// (range, start of the nearest scope) of an `it` definition
type ItDefinition = ((usize, usize), Option<usize>);

#[derive(Default)]
struct Definitions {
    scopes: Vec<Option<usize>>,
    out: Vec<Definition>,
}

impl Definitions {
    fn scope(&self) -> Option<usize> {
        self.scopes.iter().rev().find_map(|s| *s)
    }
    fn add(&mut self, name: &[u8], l: &ruby_prism::Location) {
        self.out.push((name.to_vec(), (l.start_offset(), l.end_offset()), self.scope()));
    }
}

impl<'pr> Visit<'pr> for Definitions {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        let scope = matches!(
            node,
            Node::DefNode { .. } | Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::SingletonClassNode { .. }
        );
        self.scopes.push(scope.then(|| node.location().start_offset()));
        match &node {
            Node::LocalVariableWriteNode { .. } => {
                let n = node.as_local_variable_write_node().unwrap();
                self.add(n.name().as_slice(), &node.location());
            }
            Node::LocalVariableOperatorWriteNode { .. } => {
                let n = node.as_local_variable_operator_write_node().unwrap();
                self.add(n.name().as_slice(), &n.name_loc());
            }
            Node::LocalVariableOrWriteNode { .. } => {
                let n = node.as_local_variable_or_write_node().unwrap();
                self.add(n.name().as_slice(), &n.name_loc());
            }
            Node::LocalVariableAndWriteNode { .. } => {
                let n = node.as_local_variable_and_write_node().unwrap();
                self.add(n.name().as_slice(), &n.name_loc());
            }
            _ => {}
        }
    }
    fn visit_branch_node_leave(&mut self) {
        self.scopes.pop();
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        if let Some(n) = node.as_local_variable_target_node() {
            self.add(n.name().as_slice(), &node.location());
        }
    }
}

fn definitions(node: &Node) -> Vec<Definition> {
    let mut v = Definitions::default();
    v.visit(node);
    v.out
}

/// The start of the nearest def/class/module/sclass among `ancestors[..end]`, with its index.
fn nearest_scope(ancestors: &[Node], end: usize) -> Option<usize> {
    ancestors[..end].iter().rposition(|a| {
        matches!(
            a,
            Node::DefNode { .. } | Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::SingletonClassNode { .. }
        )
    })
}

/// Parameter names (`argument_list`) of a parameters node.
fn param_names(node: &Node) -> Vec<Vec<u8>> {
    #[derive(Default)]
    struct Names(Vec<Vec<u8>>);
    impl<'pr> Visit<'pr> for Names {
        fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
            self.add(&node);
        }
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            self.add(&node);
        }
    }
    impl Names {
        fn add(&mut self, node: &Node) {
            let name = match node {
                Node::RequiredParameterNode { .. } => Some(node.as_required_parameter_node().unwrap().name()),
                Node::OptionalParameterNode { .. } => Some(node.as_optional_parameter_node().unwrap().name()),
                Node::RestParameterNode { .. } => node.as_rest_parameter_node().unwrap().name(),
                Node::RequiredKeywordParameterNode { .. } => {
                    Some(node.as_required_keyword_parameter_node().unwrap().name())
                }
                Node::OptionalKeywordParameterNode { .. } => {
                    Some(node.as_optional_keyword_parameter_node().unwrap().name())
                }
                Node::KeywordRestParameterNode { .. } => node.as_keyword_rest_parameter_node().unwrap().name(),
                Node::BlockParameterNode { .. } => node.as_block_parameter_node().unwrap().name(),
                Node::BlockLocalVariableNode { .. } => Some(node.as_block_local_variable_node().unwrap().name()),
                _ => None,
            };
            self.0.extend(name.map(|n| n.as_slice().to_vec()));
        }
    }
    let mut v = Names::default();
    v.visit(node);
    v.0
}

/// it_variable_in_scope?: an `it` argument of an enclosing block (or def) in the same scope, or
/// an `it` variable of the same scope defined before the block or inside it.
fn it_variable_in_scope(cx: &mut Cx, node: &Node) -> bool {
    let anc = cx.ancestors;
    let scope_i = nearest_scope(anc, anc.len());
    let mut owners: Vec<Option<Node>> = Vec::new();
    for a in &anc[scope_i.map_or(0, |s| s + 1)..] {
        let params = match a {
            // `each_ancestor(:block)`: numbered and `it` blocks are not `block` nodes.
            Node::BlockNode { .. } => a.as_block_node().unwrap().parameters(),
            Node::LambdaNode { .. } => a.as_lambda_node().unwrap().parameters(),
            _ => continue,
        };
        if params.as_ref().is_some_and(|p| p.as_block_parameters_node().is_some()) {
            owners.push(params);
        }
    }
    if let Some(d) = scope_i.and_then(|i| anc[i].as_def_node()) {
        owners.push(d.parameters().map(|p| p.as_node()));
    }
    if owners.iter().flatten().any(|p| param_names(p).iter().any(|n| n == b"it")) {
        return true;
    }
    let scope = scope_i.map(|i| anc[i].location().start_offset());
    let root = &anc[0];
    let all: &mut Option<Vec<ItDefinition>> = cx.state();
    let defs = all.get_or_insert_with(|| {
        definitions(root).into_iter().filter(|(n, _, _)| n == b"it").map(|(_, r, s)| (r, s)).collect()
    });
    let l = node.location();
    let (ns, ne) = (l.start_offset(), l.end_offset());
    defs.iter().any(|&(r, s)| s == scope && (r.0 < ns || (ns <= r.0 && r.1 <= ne)))
}

/// find_block_variables: the lvars named `name` in the body outside inner blocks, none when an
/// `it` variable is in scope.
fn find_block_variables(cx: &mut Cx, node: &Node, body: Option<Node>, name: &str) -> Vec<(usize, usize)> {
    let Some(body) = body else { return Vec::new() };
    if it_variable_in_scope(cx, node) {
        return Vec::new();
    }
    let ranges = inner_block_ranges(&body);
    block_variables(&body, name).into_iter().filter(|&r| !inside(&ranges, r)).collect()
}

/// keep_parameter_name?: used within an inner block, or reassigned in the body.
fn keep_parameter_name(body: Option<&Node>, name: &str) -> bool {
    let Some(body) = body else { return false };
    let ranges = inner_block_ranges(body);
    block_variables(body, name).into_iter().any(|r| inside(&ranges, r))
        || definitions(single_stmt(body).as_ref().unwrap_or(body)).iter().any(|(n, _, _)| n == name.as_bytes())
}

impl Cop for ItBlockParameter {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.4, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        // Block owners rather than BlockNode: parser visits a block before its receiver chain.
        &[kind::CALL_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let block = match node {
            Node::CallNode { .. } => node.as_call_node().unwrap().block(),
            Node::SuperNode { .. } => node.as_super_node().unwrap().block(),
            Node::ForwardingSuperNode { .. } => node.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
            _ => None,
        };
        let (params, body, opening, closing) = if let Some(l) = node.as_lambda_node() {
            (l.parameters(), l.body(), l.opening_loc(), l.closing_loc())
        } else {
            let Some(b) = block.as_ref().and_then(|b| b.as_block_node()) else { return };
            (b.parameters(), b.body(), b.opening_loc(), b.closing_loc())
        };
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("allow_single_line");
        match params.as_ref() {
            Some(Node::NumberedParametersNode { .. }) => {
                if style == "disallow" || params.unwrap().as_numbered_parameters_node().unwrap().maximum() != 1 {
                    return;
                }
                for (s, e) in find_block_variables(cx, node, body, "_1") {
                    cx.add_fix(s, e, MSG_USE, |c| c.replace(s, e, "it"));
                }
            }
            Some(Node::ItParametersNode { .. }) => match style {
                "allow_single_line" => {
                    if cx.src.line_of(opening.start_offset()) == cx.src.line_of(closing.start_offset()) {
                        return;
                    }
                    let l = node.location();
                    cx.add_with(l.start_offset(), l.end_offset(), MSG_AVOID_MULTILINE, false);
                }
                "disallow" => {
                    for (s, e) in find_block_variables(cx, node, body, "it") {
                        cx.add_with(s, e, MSG_AVOID, false);
                    }
                }
                _ => {}
            },
            Some(Node::BlockParametersNode { .. }) if style == "always" => {
                let bp = params.as_ref().unwrap().as_block_parameters_node().unwrap();
                let Some(p) = bp.parameters() else { return };
                let rest_empty = p.optionals().iter().next().is_none()
                    && parser_rest(p.rest()).is_none()
                    && p.posts().iter().next().is_none()
                    && p.keywords().iter().next().is_none()
                    && p.keyword_rest().is_none()
                    && p.block().is_none()
                    && bp.locals().iter().next().is_none();
                let reqs: Vec<Node> = p.requireds().iter().collect();
                let [arg] = reqs.as_slice() else { return };
                if !rest_empty || arg.as_required_parameter_node().is_none() {
                    return;
                }
                let name = node_text(arg);
                if keep_parameter_name(body.as_ref(), name) {
                    return;
                }
                let pl = bp.location();
                let (ps, pe) = (pl.start_offset(), pl.end_offset());
                for (s, e) in find_block_variables(cx, node, body, name) {
                    cx.add_fix(s, e, MSG_USE, |c| {
                        c.remove(ps, pe);
                        c.replace(s, e, "it");
                    });
                }
            }
            _ => {}
        }
    }
}

use crate::helpers::block_args;
use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/For";
pub static COP: For = For;
pub struct For;

const IMPLICIT_PARAMETERS: &[&str] = &["_1", "_2", "_3", "_4", "_5", "_6", "_7", "_8", "_9", "it"];

/// ForToEachCorrector#requires_parentheses?
fn requires_parentheses(node: &Node) -> bool {
    match node {
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            !is_csend(&c)
                && c.block().is_none_or(|b| b.as_block_node().is_none())
                && helpers::node_types::is_operator_method(method_name(&c))
        }
        Node::RangeNode { .. } | Node::AndNode { .. } | Node::OrNode { .. } => true,
        _ => false,
    }
}

/// `node.variable.each_node.all? { |n| n.type?(:lvasgn, :mlhs, :splat) }`
fn local_targets(node: &Node) -> bool {
    match node {
        Node::LocalVariableTargetNode { .. } | Node::ImplicitRestNode { .. } => true,
        Node::SplatNode { .. } => node.as_splat_node().unwrap().expression().is_none_or(|e| local_targets(&e)),
        Node::MultiTargetNode { .. } => {
            let m = node.as_multi_target_node().unwrap();
            m.lefts().iter().chain(m.rest()).chain(m.rights().iter()).all(|n| local_targets(&n))
        }
        _ => false,
    }
}

/// A reference to `_1`..`_9` or `it` outside any block below the `for` (`implicit_parameter?`).
#[derive(Default)]
struct ImplicitRef {
    blocks: usize,
    found: bool,
}

impl<'pr> Visit<'pr> for ImplicitRef {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.check(&node);
    }
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        self.blocks += 1;
        ruby_prism::visit_block_node(self, node);
        self.blocks -= 1;
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        self.blocks += 1;
        ruby_prism::visit_lambda_node(self, node);
        self.blocks -= 1;
    }
}

impl ImplicitRef {
    fn check(&mut self, node: &Node) {
        if self.blocks > 0 {
            return;
        }
        let name = match node {
            Node::LocalVariableReadNode { .. } => Some(node.as_local_variable_read_node().unwrap().name().as_slice()),
            Node::ItLocalVariableReadNode { .. } => Some(&b"it"[..]),
            Node::CallNode { .. } => {
                let c = node.as_call_node().unwrap();
                // A call owning a block literal sits inside that block in parser terms.
                (c.receiver().is_none() && c.arguments().is_none() && c.block().is_none()).then(|| c.name().as_slice())
            }
            _ => None,
        };
        self.found |= name.is_some_and(|n| IMPLICIT_PARAMETERS.iter().any(|p| p.as_bytes() == n));
    }
}

/// `convertible_to_for?` argument check: no lone restarg, only arg / restarg / mlhs inside.
fn plain_args(nodes: &[Node]) -> bool {
    nodes.iter().all(|n| match n {
        Node::RequiredParameterNode { .. } | Node::RestParameterNode { .. } | Node::SplatNode { .. } => true,
        Node::MultiTargetNode { .. } => plain_args(&block_args::mlhs_children(n)),
        _ => false,
    })
}

/// EachToForCorrector#loop_variable
fn loop_variable(node: &Node) -> String {
    if node.as_multi_target_node().is_none() {
        return node_text(node).to_string();
    }
    let children = block_args::mlhs_children(node);
    let vars: Vec<String> = children.iter().map(loop_variable).collect();
    let lone = children.len() == 1 && !matches!(children[0], Node::SplatNode { .. } | Node::RestParameterNode { .. });
    if lone { format!("({},)", vars.join(", ")) } else { format!("({})", vars.join(", ")) }
}

impl Cop for For {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::FOR_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let each_style = cx.cfg.str("EnforcedStyle") != Some("for");
        let src = cx.src;
        if let Some(f) = node.as_for_node() {
            if !each_style {
                return cx.correct_style_detected();
            }
            let coll = f.collection();
            let coll_src = node_text(&coll);
            let coll_src = if requires_parentheses(&coll) { format!("({coll_src})") } else { coll_src.to_string() };
            let dot = if coll.as_call_node().is_some_and(|c| is_csend(&c)) { "&." } else { "." };
            let correction = format!("{coll_src}{dot}each do |{}|", node_text(&f.index()));
            let loc = node.location();
            let (s, e) = (loc.start_offset(), loc.end_offset());
            let end = f.do_keyword_loc().map_or(coll.location().end_offset(), |d| d.end_offset());
            // convertible_to_each?: local loop variables only, and no `_1` / `it` in the body.
            let convertible = local_targets(&f.index())
                && f.statements().is_none_or(|b| {
                    let mut v = ImplicitRef::default();
                    v.visit(&b.as_node());
                    !v.found
                });
            if cx.block_runs(s, e) {
                cx.opposite_style_detected();
            }
            cx.add_fix(s, e, "Prefer `each` over `for`.", |c| {
                if convertible {
                    c.replace(s, end, &correction);
                }
            });
            return;
        }
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let (open, close) = (block.opening_loc(), block.closing_loc());
        if src.line_of(open.start_offset()) == src.line_of(close.start_offset())
            || method_name(&call) != "each"
            || call.arguments().is_some()
        {
            return;
        }
        if each_style {
            return cx.correct_style_detected();
        }
        let Some(recv) = call.receiver() else { return };
        // convertible_to_for?: not a numbered / `it` block, plain arguments only.
        let params = match block.parameters() {
            None => None,
            Some(p) => match p.as_block_parameters_node() {
                Some(bp) => Some(bp),
                None => return,
            },
        };
        let vars: Vec<Node> = params.as_ref().map(block_args::children).unwrap_or_default();
        if (vars.len() == 1 && vars[0].as_rest_parameter_node().is_some()) || !plain_args(&vars) {
            return;
        }
        if block.body().is_some_and(|b| b.as_begin_node().is_some_and(|b| b.begin_keyword_loc().is_none())) {
            return;
        }
        // do_end_block_in_collection?
        #[derive(Default)]
        struct DoEnd(bool);
        impl<'pr> Visit<'pr> for DoEnd {
            fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
                self.0 |= node.opening_loc().as_slice() == b"do";
                ruby_prism::visit_block_node(self, node);
            }
            fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
                self.0 |= node.opening_loc().as_slice() == b"do";
                ruby_prism::visit_lambda_node(self, node);
            }
        }
        let mut d = DoEnd::default();
        d.visit(&recv);
        if d.0 {
            return;
        }
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        let (end, keyword) = if vars.is_empty() {
            (open.end_offset(), format!("for _ in {} do", node_text(&recv)))
        } else {
            let p = params.as_ref().unwrap().location();
            let mut names = vars.iter().map(loop_variable).collect::<Vec<_>>().join(", ");
            // `|a, |` keeps its trailing comma.
            let args_src = &src.text[p.start_offset()..p.end_offset()];
            if args_src.strip_suffix('|').is_some_and(|a| a.trim_end_matches(|c: char| c.is_ascii_whitespace()).ends_with(',')) {
                names.push(',');
            }
            (p.end_offset(), format!("for {names} in {} do", node_text(&recv)))
        };
        // padding_needed?: a space before code that directly follows.
        let padding = |pos: usize| src.text[pos..].chars().next().is_some_and(|c| !c.is_whitespace());
        let correction = if padding(end) { format!("{keyword} ") } else { keyword };
        let braces = open.as_slice() == b"{";
        let (cs, ce) = (close.start_offset(), close.end_offset());
        let closing = if src.text[..cs].chars().next_back().is_some_and(|c| !c.is_whitespace()) { " end" } else { "end" };
        if cx.block_runs(s, e) {
            cx.opposite_style_detected();
        }
        cx.add_fix(s, e, "Prefer `for` over `each`.", |c| {
            c.replace(s, end, &correction);
            if braces {
                c.replace(cs, ce, closing);
            }
        });
    }
}

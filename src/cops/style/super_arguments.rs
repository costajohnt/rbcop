use crate::prelude::*;
use ruby_prism::{
    CaseMatchNode, DefNode, LocalVariableOrWriteNode, LocalVariableTargetNode, LocalVariableWriteNode,
    MatchPredicateNode, MatchRequiredNode, MatchWriteNode, SuperNode, Visit,
};

pub const NAME: &str = "Style/SuperArguments";
pub static COP: SuperArguments = SuperArguments;
pub struct SuperArguments;

const MSG: &str = "Call `super` without arguments and parentheses when the signature is identical.";
const MSG_INLINE_BLOCK: &str =
    "Call `super` without arguments and parentheses when all positional and keyword arguments are forwarded.";

/// A def parameter (rubocop-ast `argument_list` element).
enum DefArg<'pr> {
    /// arg / optarg
    Pos(&'pr [u8]),
    Rest(Option<&'pr [u8]>),
    /// kwarg / kwoptarg
    Kw(&'pr [u8]),
    KwRest(Option<&'pr [u8]>),
    Block(Option<&'pr [u8]>),
    Forward,
}

/// A super argument after `preprocess_super_args`.
enum SuperArg<'pr> {
    Lvar(&'pr [u8]),
    /// `*name`; None for `*` (forwarded_restarg)
    Splat(Option<&'pr [u8]>),
    /// `name: name` written as a label pair with an lvar value
    Pair(&'pr [u8]),
    /// `**name`; None for `**` (forwarded_kwrestarg)
    KwSplat(Option<&'pr [u8]>),
    /// `&x`: None for `&`; Some(`block_pass_child_name`)
    BlockPass(Option<Option<&'pr [u8]>>),
    ForwardedArgs,
    Other,
}

impl Cop for SuperArguments {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::SUPER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let sup = node.as_super_node().unwrap();
        let src = cx.src;
        let super_range = parser_super_range(&sup);
        // find_def_node: the nearest def, unless a block whose send part lacks this super is in between.
        let anc = cx.ancestors;
        let mut def = None;
        for i in (0..anc.len()).rev() {
            match kind::of(&anc[i]) {
                kind::BLOCK_NODE | kind::LAMBDA_NODE => {
                    let owner = (kind::of(&anc[i]) == kind::BLOCK_NODE && i > 0).then(|| &anc[i - 1]);
                    if !owner.is_some_and(|o| send_part_has_super(src, o, super_range)) {
                        return;
                    }
                }
                kind::DEF_NODE => {
                    def = anc[i].as_def_node();
                    break;
                }
                _ => {}
            }
        }
        let Some(def) = def else { return };
        // Zsuper passes a destructured argument as the original value, while `argument_list`
        // flattens it into its parts.
        if def.parameters().is_some_and(|p| parameter_nodes(&p).iter().any(|n| n.as_multi_target_node().is_some())) {
            return;
        }

        let def_args = def_arguments(&def);
        let super_args = super_arguments(&sup);
        // block_sends_to_super?(super_node): the parser parent is a block whose send part has this super.
        let sends_to_super = sup.block().is_some_and(|b| b.as_block_node().is_some())
            || parent_block_owner(anc).is_some_and(|o| send_part_has_super(src, o, super_range));

        let mut def_size = def_args.len();
        if sends_to_super && def_args.iter().any(|a| matches!(a, DefArg::Block(_))) {
            def_size -= 1;
        }
        if def_size != super_args.len() {
            return;
        }
        for (i, def_arg) in def_args.iter().enumerate() {
            let super_arg = super_args.get(i);
            let same = match (def_arg, super_arg) {
                (DefArg::Pos(n), Some(SuperArg::Lvar(m))) => n == m,
                (DefArg::Rest(None), Some(SuperArg::Splat(None))) => true,
                (DefArg::Rest(n), Some(SuperArg::Splat(Some(m)))) => *n == Some(*m),
                (DefArg::Kw(n), Some(SuperArg::Pair(m))) => n == m,
                (DefArg::KwRest(None), Some(SuperArg::KwSplat(None))) => true,
                (DefArg::KwRest(n), Some(SuperArg::KwSplat(Some(m)))) => *n == Some(*m),
                (DefArg::Block(_), _) if sends_to_super => true,
                (DefArg::Block(None), Some(SuperArg::BlockPass(None))) => true,
                (DefArg::Block(n), Some(SuperArg::BlockPass(Some(m)))) => {
                    *n == *m && !m.is_some_and(|m| block_reassigned(&def, m))
                }
                (DefArg::Forward, Some(SuperArg::ForwardedArgs)) => true,
                _ => false,
            };
            if !same {
                return;
            }
        }
        let message = if def_args.len() == super_args.len() { MSG } else { MSG_INLINE_BLOCK };
        let (s, e) = super_range;
        cx.add_fix(s, e, message, |c| c.replace(s, e, "super"));
    }
}

/// Parser `super` node range: without an attached `{}`/`do` block.
fn parser_super_range(sup: &SuperNode) -> (usize, usize) {
    let loc = sup.location();
    if !sup.block().is_some_and(|b| b.as_block_node().is_some()) {
        return (loc.start_offset(), loc.end_offset());
    }
    let end = sup
        .rparen_loc()
        .map(|l| l.end_offset())
        .or_else(|| sup.arguments().map(|a| a.location().end_offset()))
        .unwrap_or(sup.keyword_loc().end_offset());
    (loc.start_offset(), end)
}

/// The owner (call/super) of the block that is the super's parser parent.
fn parent_block_owner<'a, 'pr>(anc: &'a [Node<'pr>]) -> Option<&'a Node<'pr>> {
    let i = helpers::parser_parent::parser_parent_index(anc, anc.len())?;
    (kind::of(&anc[i]) == kind::BLOCK_NODE && i > 0).then(|| &anc[i - 1])
}

/// `block.send_node.each_node(:super).any?(super_node)`: structural equality, as rubocop-ast nodes.
fn send_part_has_super(src: &crate::source::Source, owner: &Node, target: (usize, usize)) -> bool {
    let mut supers = Supers(Vec::new());
    match owner {
        Node::SuperNode { .. } => {
            let s = owner.as_super_node().unwrap();
            supers.0.push(parser_super_range(&s));
            if let Some(a) = s.arguments() {
                supers.visit(&a.as_node());
            }
        }
        Node::CallNode { .. } => {
            let c = owner.as_call_node().unwrap();
            if let Some(r) = c.receiver() {
                supers.visit(&r);
            }
            if let Some(a) = c.arguments() {
                supers.visit(&a.as_node());
            }
        }
        _ => {}
    }
    supers.0.iter().any(|&r| helpers::node_equality::same(src, r, target))
}

struct Supers(Vec<(usize, usize)>);

impl<'pr> Visit<'pr> for Supers {
    fn visit_super_node(&mut self, node: &SuperNode<'pr>) {
        self.0.push(parser_super_range(node));
        ruby_prism::visit_super_node(self, node);
    }
}

fn def_arguments<'pr>(def: &DefNode<'pr>) -> Vec<DefArg<'pr>> {
    let mut out = Vec::new();
    let Some(params) = def.parameters() else { return out };
    for p in parameter_nodes(&params) {
        push_def_arg(&mut out, &p);
    }
    out
}

fn push_def_arg<'pr>(out: &mut Vec<DefArg<'pr>>, p: &Node<'pr>) {
    match p {
        Node::RequiredParameterNode { .. } => out.push(DefArg::Pos(p.as_required_parameter_node().unwrap().name().as_slice())),
        Node::OptionalParameterNode { .. } => out.push(DefArg::Pos(p.as_optional_parameter_node().unwrap().name().as_slice())),
        Node::RestParameterNode { .. } => {
            out.push(DefArg::Rest(p.as_rest_parameter_node().unwrap().name().map(|n| n.as_slice())));
        }
        Node::RequiredKeywordParameterNode { .. } => {
            out.push(DefArg::Kw(p.as_required_keyword_parameter_node().unwrap().name().as_slice()));
        }
        Node::OptionalKeywordParameterNode { .. } => {
            out.push(DefArg::Kw(p.as_optional_keyword_parameter_node().unwrap().name().as_slice()));
        }
        Node::KeywordRestParameterNode { .. } => {
            out.push(DefArg::KwRest(p.as_keyword_rest_parameter_node().unwrap().name().map(|n| n.as_slice())));
        }
        Node::BlockParameterNode { .. } => {
            out.push(DefArg::Block(p.as_block_parameter_node().unwrap().name().map(|n| n.as_slice())));
        }
        Node::ForwardingParameterNode { .. } => out.push(DefArg::Forward),
        // mlhs: its arguments, flattened (`each_descendant`)
        Node::MultiTargetNode { .. } => {
            for c in helpers::block_args::mlhs_children(p) {
                push_def_arg(out, &c);
            }
        }
        Node::SplatNode { .. } => {
            let name = p.as_splat_node().unwrap().expression().and_then(|e| e.as_required_parameter_node());
            out.push(DefArg::Rest(name.map(|n| n.name().as_slice())));
        }
        _ => {}
    }
}

fn super_arguments<'pr>(sup: &SuperNode<'pr>) -> Vec<SuperArg<'pr>> {
    let mut out = Vec::new();
    for a in sup.arguments().map(|a| a.arguments().iter().collect::<Vec<_>>()).unwrap_or_default() {
        match &a {
            Node::KeywordHashNode { .. } => {
                for e in a.as_keyword_hash_node().unwrap().elements().iter() {
                    out.push(hash_element(&e));
                }
            }
            Node::LocalVariableReadNode { .. } => out.push(SuperArg::Lvar(a.as_local_variable_read_node().unwrap().name().as_slice())),
            Node::SplatNode { .. } => out.push(match a.as_splat_node().unwrap().expression() {
                None => SuperArg::Splat(None),
                Some(e) => match e.as_local_variable_read_node() {
                    Some(l) => SuperArg::Splat(Some(l.name().as_slice())),
                    None => SuperArg::Other,
                },
            }),
            Node::ForwardingArgumentsNode { .. } => out.push(SuperArg::ForwardedArgs),
            _ => out.push(SuperArg::Other),
        }
    }
    if let Some(b) = sup.block().and_then(|b| b.as_block_argument_node()) {
        out.push(SuperArg::BlockPass(b.expression().map(|e| block_pass_child_name(&e))));
    }
    out
}

/// `block_pass.children.first.children.first` as a name: the lvar or symbol name, nil for a
/// receiverless call; anything else never equals a parameter name.
fn block_pass_child_name<'pr>(e: &Node<'pr>) -> Option<&'pr [u8]> {
    if let Some(l) = e.as_local_variable_read_node() {
        return Some(l.name().as_slice());
    }
    if let Some(s) = e.as_symbol_node() {
        return Some(s.value_loc().map_or(&b""[..], |v| v.as_slice()));
    }
    if e.as_call_node().is_some_and(|c| c.receiver().is_none()) {
        return None;
    }
    Some(b"")
}

fn hash_element<'pr>(e: &Node<'pr>) -> SuperArg<'pr> {
    if let Some(s) = e.as_assoc_splat_node() {
        return match s.value() {
            None => SuperArg::KwSplat(None),
            Some(v) => v.as_local_variable_read_node().map_or(SuperArg::Other, |l| SuperArg::KwSplat(Some(l.name().as_slice()))),
        };
    }
    let Some(pair) = e.as_assoc_node() else { return SuperArg::Other };
    // (pair (sym x) (lvar x)) with the sym's source equal to the lvar's: a label `x:` only.
    let Some(sym) = pair.key().as_symbol_node() else { return SuperArg::Other };
    let label = sym.opening_loc().is_none() && sym.closing_loc().is_some_and(|c| c.as_slice() == b":");
    let value = pair.value();
    let value = value.as_implicit_node().map_or(value, |i| i.value());
    match (label, sym.value_loc(), value.as_local_variable_read_node()) {
        (true, Some(v), Some(l)) if v.as_slice() == l.name().as_slice() => SuperArg::Pair(l.name().as_slice()),
        _ => SuperArg::Other,
    }
}

/// `def_node.each_node(:or_asgn, :lvasgn).any? { name == block_arg_name }`.
fn block_reassigned(def: &DefNode, name: &[u8]) -> bool {
    let mut v = Assigns { name, found: false };
    v.visit(&def.as_node());
    v.found
}

struct Assigns<'a> {
    name: &'a [u8],
    found: bool,
}

impl<'pr> Visit<'pr> for Assigns<'_> {
    fn visit_local_variable_write_node(&mut self, node: &LocalVariableWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &LocalVariableOrWriteNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &LocalVariableTargetNode<'pr>) {
        self.found |= node.name().as_slice() == self.name;
    }
    // Named captures (match_with_lvasgn) and pattern variables (match_var) are not lvasgn nodes.
    fn visit_match_write_node(&mut self, node: &MatchWriteNode<'pr>) {
        self.visit_call_node(&node.call());
    }
    fn visit_case_match_node(&mut self, node: &CaseMatchNode<'pr>) {
        if let Some(p) = node.predicate() {
            self.visit(&p);
        }
        for c in node.conditions().iter() {
            if let Some(i) = c.as_in_node()
                && let Some(s) = i.statements()
            {
                self.visit_statements_node(&s);
            }
        }
        if let Some(e) = node.else_clause() {
            self.visit_else_node(&e);
        }
    }
    fn visit_match_predicate_node(&mut self, node: &MatchPredicateNode<'pr>) {
        self.visit(&node.value());
    }
    fn visit_match_required_node(&mut self, node: &MatchRequiredNode<'pr>) {
        self.visit(&node.value());
    }
}

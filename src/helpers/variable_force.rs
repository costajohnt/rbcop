//! Port of RuboCop's VariableForce (local variable scopes, assignments, references and branches).
//!
//! VariableForce walks the parser-gem AST and relies on its exact shape (child indexes, parents), so
//! this module first converts the Prism tree into a small parser-shaped tree (`Tree`), keeping only
//! what the force looks at: scopes, local variable nodes, control flow, and everything else as
//! `Other` nodes whose interesting descendants are hoisted as their children.
//!
//! ponytail: `mark_assignments_as_referenced_in_loop` (loop nodes as assignment references) is not
//! ported; port it with the first cop that reads `Assignment::references` for loop nodes
//! (e.g. Lint/UselessAssignment).

use crate::node_kind as kind;
use ruby_prism::{Node, Visit};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T {
    Lvar,
    Lvasgn,
    Arg,
    Optarg,
    Restarg,
    Kwarg,
    Kwoptarg,
    Kwrestarg,
    Blockarg,
    Shadowarg,
    Args,
    Mlhs,
    Masgn,
    Splat,
    MatchWithLvasgn,
    MatchVar,
    OpAsgn,
    OrAsgn,
    AndAsgn,
    If,
    While,
    Until,
    WhilePost,
    UntilPost,
    For,
    Case,
    CaseMatch,
    And,
    Or,
    Rescue,
    Resbody,
    Ensure,
    Kwbegin,
    Begin,
    Def,
    Defs,
    Class,
    Sclass,
    Module,
    Block,
    Numblock,
    Itblock,
    Send,
    Zsuper,
    Other,
}

impl T {
    pub fn is_argument(self) -> bool {
        matches!(
            self,
            T::Arg | T::Optarg | T::Restarg | T::Kwarg | T::Kwoptarg | T::Kwrestarg | T::Blockarg | T::Shadowarg
        )
    }

    pub fn is_any_block(self) -> bool {
        matches!(self, T::Block | T::Numblock | T::Itblock)
    }

    pub fn is_scope(self) -> bool {
        matches!(self, T::Block | T::Numblock | T::Itblock | T::Class | T::Sclass | T::Defs | T::Module | T::Def)
    }

    pub fn is_shorthand_asgn(self) -> bool {
        matches!(self, T::OpAsgn | T::OrAsgn | T::AndAsgn)
    }
}

/// A parser-gem-shaped node. `children` has parser's child slots (`None` for nil or non-node
/// children such as a def's name), so slot indexes match parser's `children[i]`.
#[derive(Debug)]
pub struct PNode {
    pub t: T,
    pub start: usize,
    pub end: usize,
    /// Variable / argument / method name.
    pub name: String,
    /// `match_with_lvasgn` capture names.
    pub names: Vec<String>,
    /// `send`: number of arguments (block pass included).
    pub nargs: usize,
    /// `if` / `while` / `until` in modifier form.
    pub modifier: bool,
    pub children: Vec<Option<usize>>,
    pub parent: Option<usize>,
}

pub struct Tree {
    pub nodes: Vec<PNode>,
    pub root: Option<usize>,
}

fn loc(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

fn name_of(c: &ruby_prism::ConstantId) -> String {
    String::from_utf8_lossy(c.as_slice()).into_owned()
}

/// Descendants of a node the converter handles itself, hoisted over the nodes it does not.
struct Interesting<'pr>(Vec<Node<'pr>>);

macro_rules! stop {
    ($($f:ident : $t:ident),* $(,)?) => {
        $(fn $f(&mut self, n: &ruby_prism::$t<'pr>) { self.0.push(n.as_node()); })*
    };
}

impl<'pr> Visit<'pr> for Interesting<'pr> {
    stop!(
        visit_program_node: ProgramNode,
        visit_statements_node: StatementsNode,
        visit_parentheses_node: ParenthesesNode,
        visit_begin_node: BeginNode,
        visit_rescue_modifier_node: RescueModifierNode,
        visit_else_node: ElseNode,
        visit_if_node: IfNode,
        visit_unless_node: UnlessNode,
        visit_while_node: WhileNode,
        visit_until_node: UntilNode,
        visit_for_node: ForNode,
        visit_case_node: CaseNode,
        visit_case_match_node: CaseMatchNode,
        visit_and_node: AndNode,
        visit_or_node: OrNode,
        visit_local_variable_read_node: LocalVariableReadNode,
        visit_it_local_variable_read_node: ItLocalVariableReadNode,
        visit_local_variable_write_node: LocalVariableWriteNode,
        visit_local_variable_target_node: LocalVariableTargetNode,
        visit_local_variable_or_write_node: LocalVariableOrWriteNode,
        visit_local_variable_and_write_node: LocalVariableAndWriteNode,
        visit_local_variable_operator_write_node: LocalVariableOperatorWriteNode,
        visit_instance_variable_or_write_node: InstanceVariableOrWriteNode,
        visit_instance_variable_and_write_node: InstanceVariableAndWriteNode,
        visit_instance_variable_operator_write_node: InstanceVariableOperatorWriteNode,
        visit_class_variable_or_write_node: ClassVariableOrWriteNode,
        visit_class_variable_and_write_node: ClassVariableAndWriteNode,
        visit_class_variable_operator_write_node: ClassVariableOperatorWriteNode,
        visit_global_variable_or_write_node: GlobalVariableOrWriteNode,
        visit_global_variable_and_write_node: GlobalVariableAndWriteNode,
        visit_global_variable_operator_write_node: GlobalVariableOperatorWriteNode,
        visit_constant_or_write_node: ConstantOrWriteNode,
        visit_constant_and_write_node: ConstantAndWriteNode,
        visit_constant_operator_write_node: ConstantOperatorWriteNode,
        visit_constant_path_or_write_node: ConstantPathOrWriteNode,
        visit_constant_path_and_write_node: ConstantPathAndWriteNode,
        visit_constant_path_operator_write_node: ConstantPathOperatorWriteNode,
        visit_call_or_write_node: CallOrWriteNode,
        visit_call_and_write_node: CallAndWriteNode,
        visit_call_operator_write_node: CallOperatorWriteNode,
        visit_index_or_write_node: IndexOrWriteNode,
        visit_index_and_write_node: IndexAndWriteNode,
        visit_index_operator_write_node: IndexOperatorWriteNode,
        visit_multi_write_node: MultiWriteNode,
        visit_match_write_node: MatchWriteNode,
        visit_def_node: DefNode,
        visit_call_node: CallNode,
        visit_lambda_node: LambdaNode,
        visit_forwarding_super_node: ForwardingSuperNode,
        visit_super_node: SuperNode,
        visit_class_node: ClassNode,
        visit_module_node: ModuleNode,
        visit_singleton_class_node: SingletonClassNode,
        visit_embedded_statements_node: EmbeddedStatementsNode,
        visit_implicit_node: ImplicitNode,
    );
}

/// A non-local `||=` / `&&=` / `op=` node: the parser type it becomes, the nodes inside its
/// left-hand side (receiver, index arguments, constant path) and its value.
fn shorthand<'pr>(node: &Node<'pr>) -> Option<(T, Vec<Node<'pr>>, Node<'pr>)> {
    macro_rules! var {
        ($as:ident, $t:expr) => {{
            let n = node.$as().unwrap();
            ($t, Vec::new(), n.value())
        }};
    }
    macro_rules! call {
        ($as:ident, $t:expr) => {{
            let n = node.$as().unwrap();
            ($t, n.receiver().into_iter().collect(), n.value())
        }};
    }
    macro_rules! index {
        ($as:ident, $t:expr) => {{
            let n = node.$as().unwrap();
            let mut lhs: Vec<Node<'pr>> = n.receiver().into_iter().collect();
            lhs.extend(n.arguments().into_iter().flat_map(|a| a.arguments().iter()));
            lhs.extend(n.block().map(|b| b.as_node()));
            ($t, lhs, n.value())
        }};
    }
    macro_rules! path {
        ($as:ident, $t:expr) => {{
            let n = node.$as().unwrap();
            ($t, vec![n.target().as_node()], n.value())
        }};
    }
    Some(match kind::of(node) {
        kind::INSTANCE_VARIABLE_OR_WRITE_NODE => var!(as_instance_variable_or_write_node, T::OrAsgn),
        kind::INSTANCE_VARIABLE_AND_WRITE_NODE => var!(as_instance_variable_and_write_node, T::AndAsgn),
        kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE => var!(as_instance_variable_operator_write_node, T::OpAsgn),
        kind::CLASS_VARIABLE_OR_WRITE_NODE => var!(as_class_variable_or_write_node, T::OrAsgn),
        kind::CLASS_VARIABLE_AND_WRITE_NODE => var!(as_class_variable_and_write_node, T::AndAsgn),
        kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE => var!(as_class_variable_operator_write_node, T::OpAsgn),
        kind::GLOBAL_VARIABLE_OR_WRITE_NODE => var!(as_global_variable_or_write_node, T::OrAsgn),
        kind::GLOBAL_VARIABLE_AND_WRITE_NODE => var!(as_global_variable_and_write_node, T::AndAsgn),
        kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE => var!(as_global_variable_operator_write_node, T::OpAsgn),
        kind::CONSTANT_OR_WRITE_NODE => var!(as_constant_or_write_node, T::OrAsgn),
        kind::CONSTANT_AND_WRITE_NODE => var!(as_constant_and_write_node, T::AndAsgn),
        kind::CONSTANT_OPERATOR_WRITE_NODE => var!(as_constant_operator_write_node, T::OpAsgn),
        kind::CONSTANT_PATH_OR_WRITE_NODE => path!(as_constant_path_or_write_node, T::OrAsgn),
        kind::CONSTANT_PATH_AND_WRITE_NODE => path!(as_constant_path_and_write_node, T::AndAsgn),
        kind::CONSTANT_PATH_OPERATOR_WRITE_NODE => path!(as_constant_path_operator_write_node, T::OpAsgn),
        kind::CALL_OR_WRITE_NODE => call!(as_call_or_write_node, T::OrAsgn),
        kind::CALL_AND_WRITE_NODE => call!(as_call_and_write_node, T::AndAsgn),
        kind::CALL_OPERATOR_WRITE_NODE => call!(as_call_operator_write_node, T::OpAsgn),
        kind::INDEX_OR_WRITE_NODE => index!(as_index_or_write_node, T::OrAsgn),
        kind::INDEX_AND_WRITE_NODE => index!(as_index_and_write_node, T::AndAsgn),
        kind::INDEX_OPERATOR_WRITE_NODE => index!(as_index_operator_write_node, T::OpAsgn),
        _ => return None,
    })
}

impl Tree {
    pub fn build(program: &ruby_prism::ProgramNode) -> Tree {
        let mut tree = Tree { nodes: Vec::new(), root: None };
        tree.root = tree.stmts(Some(program.statements()));
        tree
    }

    fn mk(&mut self, t: T, (start, end): (usize, usize)) -> usize {
        self.nodes.push(PNode {
            t,
            start,
            end,
            name: String::new(),
            names: Vec::new(),
            nargs: 0,
            modifier: false,
            children: Vec::new(),
            parent: None,
        });
        self.nodes.len() - 1
    }

    fn named(&mut self, t: T, range: (usize, usize), name: String) -> usize {
        let i = self.mk(t, range);
        self.nodes[i].name = name;
        i
    }

    fn kid(&mut self, parent: usize, child: Option<usize>) {
        if let Some(c) = child {
            self.nodes[c].parent = Some(parent);
        }
        self.nodes[parent].children.push(child);
    }

    fn conv_kid(&mut self, parent: usize, node: Option<Node>) {
        let c = node.and_then(|n| self.conv(&n));
        self.kid(parent, c);
    }

    fn stmts(&mut self, st: Option<ruby_prism::StatementsNode>) -> Option<usize> {
        let body: Vec<Node> = st.map(|s| s.body().iter().collect()).unwrap_or_default();
        match body.len() {
            0 => None,
            1 => self.conv(&body[0]),
            _ => {
                let b = self.mk(T::Begin, (loc(&body[0]).0, loc(body.last().unwrap()).1));
                for n in &body {
                    let c = self.conv(n);
                    self.kid(b, c);
                }
                Some(b)
            }
        }
    }

    /// A body slot: StatementsNode, or an implicit BeginNode carrying rescue/ensure.
    fn body(&mut self, node: Option<Node>) -> Option<usize> {
        self.conv(&node?)
    }

    fn else_body(&mut self, node: Option<Node>) -> Option<usize> {
        let node = node?;
        match node.as_else_node() {
            Some(e) => self.stmts(e.statements()),
            None => self.conv(&node),
        }
    }

    /// Target of masgn / for / rescue (lvasgn, mlhs, splat).
    fn target(&mut self, node: &Node) -> Option<usize> {
        let range = loc(node);
        match kind::of(node) {
            kind::LOCAL_VARIABLE_TARGET_NODE => {
                Some(self.named(T::Lvasgn, range, name_of(&node.as_local_variable_target_node().unwrap().name())))
            }
            kind::MULTI_TARGET_NODE => {
                let m = node.as_multi_target_node().unwrap();
                let i = self.mk(T::Mlhs, range);
                self.targets(i, m.lefts().iter(), crate::ast::parser_rest(m.rest()), m.rights().iter());
                Some(i)
            }
            kind::SPLAT_NODE => {
                let i = self.mk(T::Splat, range);
                if let Some(e) = node.as_splat_node().unwrap().expression() {
                    let c = self.target(&e);
                    self.kid(i, c);
                }
                Some(i)
            }
            _ => self.conv(node),
        }
    }

    fn targets<'a>(
        &mut self,
        parent: usize,
        lefts: impl Iterator<Item = Node<'a>>,
        rest: Option<Node<'a>>,
        rights: impl Iterator<Item = Node<'a>>,
    ) {
        for n in lefts.chain(rest).chain(rights) {
            if let Some(c) = self.target(&n) {
                self.kid(parent, Some(c));
            }
        }
    }

    /// A parameter (inside `args` or a destructuring mlhs).
    fn param(&mut self, node: &Node) -> Option<usize> {
        let range = loc(node);
        let opt = |c: Option<ruby_prism::ConstantId>| c.map(|c| name_of(&c)).unwrap_or_default();
        Some(match kind::of(node) {
            kind::REQUIRED_PARAMETER_NODE => {
                self.named(T::Arg, range, name_of(&node.as_required_parameter_node().unwrap().name()))
            }
            kind::OPTIONAL_PARAMETER_NODE => {
                let p = node.as_optional_parameter_node().unwrap();
                let i = self.named(T::Optarg, range, name_of(&p.name()));
                self.conv_kid(i, Some(p.value()));
                i
            }
            kind::REST_PARAMETER_NODE => self.named(T::Restarg, range, opt(node.as_rest_parameter_node().unwrap().name())),
            kind::REQUIRED_KEYWORD_PARAMETER_NODE => {
                self.named(T::Kwarg, range, name_of(&node.as_required_keyword_parameter_node().unwrap().name()))
            }
            kind::OPTIONAL_KEYWORD_PARAMETER_NODE => {
                let p = node.as_optional_keyword_parameter_node().unwrap();
                let i = self.named(T::Kwoptarg, range, name_of(&p.name()));
                self.conv_kid(i, Some(p.value()));
                i
            }
            kind::KEYWORD_REST_PARAMETER_NODE => {
                self.named(T::Kwrestarg, range, opt(node.as_keyword_rest_parameter_node().unwrap().name()))
            }
            kind::BLOCK_PARAMETER_NODE => self.named(T::Blockarg, range, opt(node.as_block_parameter_node().unwrap().name())),
            kind::BLOCK_LOCAL_VARIABLE_NODE => {
                self.named(T::Shadowarg, range, name_of(&node.as_block_local_variable_node().unwrap().name()))
            }
            kind::MULTI_TARGET_NODE => {
                let m = node.as_multi_target_node().unwrap();
                let i = self.mk(T::Mlhs, range);
                for n in m.lefts().iter().chain(crate::ast::parser_rest(m.rest())).chain(m.rights().iter()) {
                    let c = match n.as_splat_node() {
                        Some(s) => {
                            let name = s
                                .expression()
                                .and_then(|e| e.as_required_parameter_node().map(|r| name_of(&r.name())))
                                .unwrap_or_default();
                            Some(self.named(T::Restarg, loc(&n), name))
                        }
                        None => self.param(&n),
                    };
                    if c.is_some() {
                        self.kid(i, c);
                    }
                }
                i
            }
            _ => self.mk(T::Other, range),
        })
    }

    fn args(&mut self, params: Option<ruby_prism::ParametersNode>, locals: Vec<Node>, range: (usize, usize)) -> usize {
        let a = self.mk(T::Args, range);
        let mut all: Vec<Node> = Vec::new();
        if let Some(p) = params {
            all.extend(p.requireds().iter());
            all.extend(p.optionals().iter());
            all.extend(crate::ast::parser_rest(p.rest()));
            all.extend(p.posts().iter());
            all.extend(p.keywords().iter());
            all.extend(p.keyword_rest());
            all.extend(p.block().map(|b| b.as_node()));
        }
        all.extend(locals);
        all.sort_by_key(|n| n.location().start_offset());
        for n in &all {
            if let Some(c) = self.param(n) {
                self.kid(a, Some(c));
            }
        }
        a
    }

    /// `(block send args body)` for a call / lambda / super with a literal block.
    fn block(&mut self, send: usize, block: &ruby_prism::BlockNode, range: (usize, usize)) -> usize {
        self.block_parts(send, block.parameters(), block.body(), range)
    }

    fn block_parts(&mut self, send: usize, params: Option<Node>, body: Option<Node>, range: (usize, usize)) -> usize {
        let (t, args) = match &params {
            Some(p) if p.as_numbered_parameters_node().is_some() => (T::Numblock, None),
            Some(p) if p.as_it_parameters_node().is_some() => (T::Itblock, None),
            Some(p) => {
                let bp = p.as_block_parameters_node().unwrap();
                (T::Block, Some((bp.parameters(), bp.locals().iter().collect::<Vec<_>>(), loc(p))))
            }
            None => (T::Block, Some((None, Vec::new(), (range.1, range.1)))),
        };
        let b = self.mk(t, range);
        self.kid(b, Some(send));
        let a = args.map(|(p, l, r)| self.args(p, l, r));
        self.kid(b, a);
        let body = self.body(body);
        self.kid(b, body);
        b
    }

    fn begin_body(&mut self, b: &ruby_prism::BeginNode, range: (usize, usize)) -> Option<usize> {
        let mut body = self.stmts(b.statements());
        if let Some(r) = b.rescue_clause() {
            let start = body.map_or(r.keyword_loc().start_offset(), |m| self.nodes[m].start);
            let rn = self.mk(T::Rescue, (start, range.1));
            self.kid(rn, body);
            let mut cur = Some(r);
            while let Some(rc) = cur {
                let rb = self.mk(T::Resbody, (rc.keyword_loc().start_offset(), rc.location().end_offset()));
                let exceptions: Vec<Node> = rc.exceptions().iter().collect();
                if exceptions.is_empty() {
                    self.kid(rb, None);
                } else {
                    let arr = self.mk(T::Other, (loc(&exceptions[0]).0, loc(exceptions.last().unwrap()).1));
                    for e in &exceptions {
                        let c = self.conv(e);
                        self.kid(arr, c);
                    }
                    self.kid(rb, Some(arr));
                }
                let reference = rc.reference().and_then(|r| self.target(&r));
                self.kid(rb, reference);
                let st = self.stmts(rc.statements());
                self.kid(rb, st);
                self.kid(rn, Some(rb));
                cur = rc.subsequent();
            }
            let els = b.else_clause().and_then(|e| self.stmts(e.statements()));
            self.kid(rn, els);
            body = Some(rn);
        }
        if let Some(en) = b.ensure_clause() {
            let start = body.map_or(en.ensure_keyword_loc().start_offset(), |m| self.nodes[m].start);
            let e = self.mk(T::Ensure, (start, range.1));
            self.kid(e, body);
            let eb = self.stmts(en.statements());
            self.kid(e, eb);
            body = Some(e);
        }
        body
    }

    fn conv(&mut self, node: &Node) -> Option<usize> {
        let range = loc(node);
        if let Some((t, lhs_nodes, value)) = shorthand(node) {
            let i = self.mk(t, range);
            let lhs = self.mk(T::Other, (range.0, loc(&value).0));
            for k in &lhs_nodes {
                let c = self.conv(k);
                self.kid(lhs, c);
            }
            self.kid(i, Some(lhs));
            if t == T::OpAsgn {
                self.kid(i, None);
            }
            self.conv_kid(i, Some(value));
            return Some(i);
        }
        Some(match kind::of(node) {
            kind::PROGRAM_NODE => return self.stmts(Some(node.as_program_node().unwrap().statements())),
            kind::STATEMENTS_NODE => return self.stmts(node.as_statements_node()),
            kind::ELSE_NODE => return self.stmts(node.as_else_node().unwrap().statements()),
            kind::IMPLICIT_NODE => return self.conv(&node.as_implicit_node().unwrap().value()),
            kind::PARENTHESES_NODE => {
                let i = self.mk(T::Begin, range);
                match node.as_parentheses_node().unwrap().body() {
                    Some(b) if b.as_statements_node().is_some() => {
                        for n in b.as_statements_node().unwrap().body().iter() {
                            let c = self.conv(&n);
                            self.kid(i, c);
                        }
                    }
                    b => self.conv_kid(i, b),
                }
                i
            }
            kind::EMBEDDED_STATEMENTS_NODE => {
                let i = self.mk(T::Begin, range);
                if let Some(st) = node.as_embedded_statements_node().unwrap().statements() {
                    for n in st.body().iter() {
                        let c = self.conv(&n);
                        self.kid(i, c);
                    }
                }
                i
            }
            kind::BEGIN_NODE => {
                let b = node.as_begin_node().unwrap();
                if b.begin_keyword_loc().is_none() {
                    return self.begin_body(&b, range);
                }
                let k = self.mk(T::Kwbegin, range);
                if b.rescue_clause().is_none() && b.ensure_clause().is_none() {
                    if let Some(st) = b.statements() {
                        for n in st.body().iter() {
                            let c = self.conv(&n);
                            self.kid(k, c);
                        }
                    }
                } else {
                    let inner = self.begin_body(&b, range);
                    self.kid(k, inner);
                }
                k
            }
            kind::RESCUE_MODIFIER_NODE => {
                let m = node.as_rescue_modifier_node().unwrap();
                let r = self.mk(T::Rescue, range);
                self.conv_kid(r, Some(m.expression()));
                let rb = self.mk(T::Resbody, (m.keyword_loc().start_offset(), range.1));
                self.kid(rb, None);
                self.kid(rb, None);
                self.conv_kid(rb, Some(m.rescue_expression()));
                self.kid(r, Some(rb));
                self.kid(r, None);
                r
            }
            kind::IF_NODE => {
                let n = node.as_if_node().unwrap();
                let i = self.mk(T::If, range);
                self.nodes[i].modifier =
                    n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if") && n.end_keyword_loc().is_none();
                self.conv_kid(i, Some(n.predicate()));
                let then = self.stmts(n.statements());
                self.kid(i, then);
                let els = self.else_body(n.subsequent());
                self.kid(i, els);
                i
            }
            kind::UNLESS_NODE => {
                let n = node.as_unless_node().unwrap();
                let i = self.mk(T::If, range);
                self.nodes[i].modifier = n.end_keyword_loc().is_none();
                self.conv_kid(i, Some(n.predicate()));
                let els = n.else_clause().and_then(|e| self.stmts(e.statements()));
                self.kid(i, els);
                let then = self.stmts(n.statements());
                self.kid(i, then);
                i
            }
            kind::WHILE_NODE | kind::UNTIL_NODE => {
                let (post, closing, pred, st, (t, tpost)) = match node.as_while_node() {
                    Some(w) => (w.is_begin_modifier(), w.closing_loc(), w.predicate(), w.statements(), (T::While, T::WhilePost)),
                    None => {
                        let u = node.as_until_node().unwrap();
                        (u.is_begin_modifier(), u.closing_loc(), u.predicate(), u.statements(), (T::Until, T::UntilPost))
                    }
                };
                let i = self.mk(if post { tpost } else { t }, range);
                self.nodes[i].modifier = !post && closing.is_none();
                self.conv_kid(i, Some(pred));
                let body = self.stmts(st);
                self.kid(i, body);
                i
            }
            kind::FOR_NODE => {
                let f = node.as_for_node().unwrap();
                let i = self.mk(T::For, range);
                let var = self.target(&f.index());
                self.kid(i, var);
                self.conv_kid(i, Some(f.collection()));
                let body = self.stmts(f.statements());
                self.kid(i, body);
                i
            }
            kind::CASE_NODE => {
                let c = node.as_case_node().unwrap();
                let i = self.mk(T::Case, range);
                self.conv_kid(i, c.predicate());
                for w in c.conditions().iter() {
                    let wn = w.as_when_node().unwrap();
                    let wi = self.mk(T::Other, loc(&w));
                    for cond in wn.conditions().iter() {
                        let x = self.conv(&cond);
                        self.kid(wi, x);
                    }
                    let body = self.stmts(wn.statements());
                    self.kid(wi, body);
                    self.kid(i, Some(wi));
                }
                let els = c.else_clause().and_then(|e| self.stmts(e.statements()));
                self.kid(i, els);
                i
            }
            kind::CASE_MATCH_NODE => {
                let c = node.as_case_match_node().unwrap();
                let i = self.mk(T::CaseMatch, range);
                self.conv_kid(i, c.predicate());
                for w in c.conditions().iter() {
                    let inn = w.as_in_node().unwrap();
                    let wi = self.mk(T::Other, loc(&w));
                    let pattern = inn.pattern();
                    // `in pat if cond`: Prism wraps the pattern in an IfNode/UnlessNode.
                    let guarded = match kind::of(&pattern) {
                        kind::IF_NODE => {
                            let g = pattern.as_if_node().unwrap();
                            Some((g.statements(), g.predicate()))
                        }
                        kind::UNLESS_NODE => {
                            let g = pattern.as_unless_node().unwrap();
                            Some((g.statements(), g.predicate()))
                        }
                        _ => None,
                    };
                    match guarded {
                        Some((pat, cond)) => {
                            let p = self.stmts(pat);
                            self.kid(wi, p);
                            let g = self.mk(T::Other, loc(&cond));
                            self.conv_kid(g, Some(cond));
                            self.kid(wi, Some(g));
                        }
                        None => {
                            self.conv_kid(wi, Some(pattern));
                            self.kid(wi, None);
                        }
                    }
                    let body = self.stmts(inn.statements());
                    self.kid(wi, body);
                    self.kid(i, Some(wi));
                }
                let els = c.else_clause().and_then(|e| self.stmts(e.statements()));
                self.kid(i, els);
                i
            }
            kind::AND_NODE | kind::OR_NODE => {
                let (t, l, r) = match node.as_and_node() {
                    Some(a) => (T::And, a.left(), a.right()),
                    None => {
                        let o = node.as_or_node().unwrap();
                        (T::Or, o.left(), o.right())
                    }
                };
                let i = self.mk(t, range);
                self.conv_kid(i, Some(l));
                self.conv_kid(i, Some(r));
                i
            }
            kind::LOCAL_VARIABLE_READ_NODE => {
                self.named(T::Lvar, range, name_of(&node.as_local_variable_read_node().unwrap().name()))
            }
            kind::IT_LOCAL_VARIABLE_READ_NODE => self.named(T::Lvar, range, "it".into()),
            kind::LOCAL_VARIABLE_WRITE_NODE => {
                let w = node.as_local_variable_write_node().unwrap();
                let i = self.named(T::Lvasgn, range, name_of(&w.name()));
                self.conv_kid(i, Some(w.value()));
                i
            }
            // Outside masgn / for / rescue (handled by `target`) a target is a pattern variable.
            kind::LOCAL_VARIABLE_TARGET_NODE => {
                self.named(T::MatchVar, range, name_of(&node.as_local_variable_target_node().unwrap().name()))
            }
            kind::LOCAL_VARIABLE_OR_WRITE_NODE
            | kind::LOCAL_VARIABLE_AND_WRITE_NODE
            | kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE => {
                let (t, name, name_loc, value) = if let Some(w) = node.as_local_variable_or_write_node() {
                    (T::OrAsgn, w.name(), w.name_loc(), w.value())
                } else if let Some(w) = node.as_local_variable_and_write_node() {
                    (T::AndAsgn, w.name(), w.name_loc(), w.value())
                } else {
                    let w = node.as_local_variable_operator_write_node().unwrap();
                    (T::OpAsgn, w.name(), w.name_loc(), w.value())
                };
                let i = self.mk(t, range);
                let lhs = self.named(T::Lvasgn, (name_loc.start_offset(), name_loc.end_offset()), name_of(&name));
                self.kid(i, Some(lhs));
                if t == T::OpAsgn {
                    self.kid(i, None);
                }
                self.conv_kid(i, Some(value));
                i
            }
            kind::MULTI_WRITE_NODE => {
                let m = node.as_multi_write_node().unwrap();
                let i = self.mk(T::Masgn, range);
                let first = m.lefts().iter().chain(crate::ast::parser_rest(m.rest())).chain(m.rights().iter()).map(|n| loc(&n)).collect::<Vec<_>>();
                let mlhs_range = match (m.lparen_loc(), m.rparen_loc()) {
                    (Some(l), Some(r)) => (l.start_offset(), r.end_offset()),
                    _ => (first.first().map_or(range.0, |r| r.0), first.last().map_or(range.0, |r| r.1)),
                };
                let mlhs = self.mk(T::Mlhs, mlhs_range);
                self.targets(mlhs, m.lefts().iter(), crate::ast::parser_rest(m.rest()), m.rights().iter());
                self.kid(i, Some(mlhs));
                self.conv_kid(i, Some(m.value()));
                i
            }
            kind::MATCH_WRITE_NODE => {
                let m = node.as_match_write_node().unwrap();
                let i = self.mk(T::MatchWithLvasgn, range);
                self.nodes[i].names = m
                    .targets()
                    .iter()
                    .filter_map(|t| t.as_local_variable_target_node().map(|t| name_of(&t.name())))
                    .collect();
                let call = m.call();
                self.conv_kid(i, call.receiver());
                let arg = call.arguments().and_then(|a| a.arguments().iter().next());
                self.conv_kid(i, arg);
                i
            }
            kind::DEF_NODE => {
                let d = node.as_def_node().unwrap();
                let i = self.mk(if d.receiver().is_some() { T::Defs } else { T::Def }, range);
                self.nodes[i].name = name_of(&d.name());
                if let Some(r) = d.receiver() {
                    self.conv_kid(i, Some(r));
                }
                self.kid(i, None);
                let arange = match (d.lparen_loc(), d.rparen_loc()) {
                    (Some(l), Some(r)) => (l.start_offset(), r.end_offset()),
                    _ => d.parameters().map_or((range.0, range.0), |p| loc(&p.as_node())),
                };
                let a = self.args(d.parameters(), Vec::new(), arange);
                self.kid(i, Some(a));
                let body = self.body(d.body());
                self.kid(i, body);
                i
            }
            kind::CALL_NODE => {
                let c = node.as_call_node().unwrap();
                let s = self.named(T::Send, crate::ast::send_range(&c), name_of(&c.name()));
                self.conv_kid(s, c.receiver());
                let mut n = 0;
                if let Some(args) = c.arguments() {
                    for a in args.arguments().iter() {
                        let x = self.conv(&a);
                        self.kid(s, x);
                        n += 1;
                    }
                }
                match c.block() {
                    Some(b) if b.as_block_node().is_some() => {
                        self.nodes[s].nargs = n;
                        return Some(self.block(s, &b.as_block_node().unwrap(), range));
                    }
                    Some(b) => {
                        self.conv_kid(s, Some(b));
                        n += 1;
                    }
                    None => {}
                }
                self.nodes[s].nargs = n;
                s
            }
            kind::LAMBDA_NODE => {
                let l = node.as_lambda_node().unwrap();
                let op = l.operator_loc();
                let s = self.named(T::Send, (op.start_offset(), op.end_offset()), "lambda".into());
                return Some(self.block_parts(s, l.parameters(), l.body(), range));
            }
            kind::FORWARDING_SUPER_NODE => {
                let f = node.as_forwarding_super_node().unwrap();
                match f.block() {
                    Some(b) => {
                        let z = self.mk(T::Zsuper, (range.0, range.0 + 5));
                        return Some(self.block(z, &b, range));
                    }
                    None => self.mk(T::Zsuper, range),
                }
            }
            kind::SUPER_NODE => {
                let sn = node.as_super_node().unwrap();
                let block = sn.block().and_then(|b| b.as_block_node());
                let end = sn.rparen_loc().map(|l| l.end_offset()).or_else(|| sn.arguments().map(|a| loc(&a.as_node()).1));
                let s = self.mk(T::Other, if block.is_some() { (range.0, end.unwrap_or(range.0 + 5)) } else { range });
                if let Some(args) = sn.arguments() {
                    for a in args.arguments().iter() {
                        let x = self.conv(&a);
                        self.kid(s, x);
                    }
                }
                match (block, sn.block()) {
                    (Some(b), _) => return Some(self.block(s, &b, range)),
                    (None, Some(bp)) => self.conv_kid(s, Some(bp)),
                    _ => {}
                }
                s
            }
            kind::CLASS_NODE => {
                let c = node.as_class_node().unwrap();
                let i = self.mk(T::Class, range);
                self.conv_kid(i, Some(c.constant_path()));
                self.conv_kid(i, c.superclass());
                let body = self.body(c.body());
                self.kid(i, body);
                i
            }
            kind::MODULE_NODE => {
                let m = node.as_module_node().unwrap();
                let i = self.mk(T::Module, range);
                self.conv_kid(i, Some(m.constant_path()));
                let body = self.body(m.body());
                self.kid(i, body);
                i
            }
            kind::SINGLETON_CLASS_NODE => {
                let s = node.as_singleton_class_node().unwrap();
                let i = self.mk(T::Sclass, range);
                self.conv_kid(i, Some(s.expression()));
                let body = self.body(s.body());
                self.kid(i, body);
                i
            }
            _ => {
                let i = self.mk(T::Other, range);
                let mut kids = Interesting(Vec::new());
                kids.visit(node);
                for k in &kids.0 {
                    // ponytail: a kind listed in `Interesting` but not handled above would
                    // collect itself; treat it as a leaf.
                    if loc(k) == range && kind::of(k) == kind::of(node) {
                        continue;
                    }
                    let c = self.conv(k);
                    self.kid(i, c);
                }
                i
            }
        })
    }

    pub fn parent(&self, n: usize) -> Option<usize> {
        self.nodes[n].parent
    }

    pub fn t(&self, n: usize) -> T {
        self.nodes[n].t
    }

    pub fn child(&self, n: usize, i: usize) -> Option<usize> {
        self.nodes[n].children.get(i).copied().flatten()
    }

    pub fn last_child(&self, n: usize) -> Option<usize> {
        self.nodes[n].children.last().copied().flatten()
    }

    pub fn sibling_index(&self, n: usize) -> Option<usize> {
        let p = self.parent(n)?;
        self.nodes[p].children.iter().position(|c| *c == Some(n))
    }

    pub fn ancestors(&self, n: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(self.parent(n), move |&p| self.parent(p))
    }

    pub fn range(&self, n: usize) -> (usize, usize) {
        (self.nodes[n].start, self.nodes[n].end)
    }

    /// `n` and all its descendants.
    pub fn each_node(&self, n: usize, f: &mut impl FnMut(usize)) {
        f(n);
        for c in self.nodes[n].children.iter().flatten() {
            self.each_node(*c, f);
        }
    }

    fn covers(&self, container: usize, node: usize) -> bool {
        let (c, n) = (self.range(container), self.range(node));
        container == node || (c.0 <= n.0 && n.1 <= c.1)
    }
}

/// `Branch` (control node + the child of it that the branch is).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Branch {
    pub control: usize,
    pub child: usize,
}

/// A scope as VariableForce's `Scope`: its node and whether it is the naked top level.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScopeRef {
    pub node: usize,
    pub naked: bool,
}

fn outer_indices(t: T) -> Option<std::ops::RangeInclusive<usize>> {
    match t {
        T::Defs | T::Module | T::Sclass | T::Block => Some(0..=0),
        T::Class => Some(0..=1),
        _ => None,
    }
}

impl Tree {
    /// `Scope#include?`
    pub fn scope_include(&self, scope: ScopeRef, target: usize) -> bool {
        // belong_to_outer_scope?
        if (!scope.naked && target == scope.node) || self.ancestors(scope.node).any(|a| a == target) {
            return false;
        }
        let parent = self.parent(target);
        if parent == Some(scope.node) {
            if let (Some(r), Some(i)) = (outer_indices(self.t(scope.node)), self.sibling_index(target))
                && r.contains(&i)
            {
                return false;
            }
            return true;
        }
        // belong_to_inner_scope?
        let Some(p) = parent else { return true };
        if !self.t(p).is_scope() {
            return true;
        }
        match (outer_indices(self.t(p)), self.sibling_index(target)) {
            (Some(r), Some(i)) => r.contains(&i),
            _ => false,
        }
    }

    fn always_run(&self, b: Branch) -> bool {
        let c = |i| self.child(b.control, i) == Some(b.child);
        match self.t(b.control) {
            T::If | T::While | T::Until | T::WhilePost | T::UntilPost => c(0),
            T::Case | T::CaseMatch => c(0),
            T::For => c(0) || c(1),
            T::And | T::Or | T::AndAsgn | T::OrAsgn | T::OpAsgn => c(0),
            T::Rescue => false,
            T::Ensure => self.last_child(b.control) == Some(b.child),
            _ => true,
        }
    }

    fn has_branches(t: T) -> bool {
        matches!(
            t,
            T::If
                | T::While
                | T::Until
                | T::WhilePost
                | T::UntilPost
                | T::Case
                | T::CaseMatch
                | T::For
                | T::And
                | T::Or
                | T::AndAsgn
                | T::OrAsgn
                | T::OpAsgn
                | T::Rescue
                | T::Ensure
        )
    }

    /// `Branch.of(target_node, scope:)`
    pub fn branch_of(&self, target: usize, scope: ScopeRef) -> Option<Branch> {
        for node in std::iter::once(target).chain(self.ancestors(target)) {
            let parent = self.parent(node)?;
            if !self.scope_include(scope, node) {
                return None;
            }
            if !Self::has_branches(self.t(parent)) {
                continue;
            }
            let b = Branch { control: parent, child: node };
            if !self.always_run(b) {
                return Some(b);
            }
        }
        None
    }

    /// `may_jump_to_other_branch?` / `may_run_incompletely?` (exception handler main body).
    pub fn main_body_of_handler(&self, b: Branch) -> bool {
        matches!(self.t(b.control), T::Rescue | T::Ensure) && self.child(b.control, 0) == Some(b.child)
    }

    pub fn exclusive_with(&self, b: Branch, other: Option<Branch>, scope: ScopeRef) -> bool {
        let Some(other) = other else { return false };
        if self.main_body_of_handler(b) {
            return false;
        }
        let mut anc = Some(other);
        while let Some(o) = anc {
            if b.control == o.control {
                return b.child != o.child;
            }
            anc = self.branch_of(o.control, scope);
        }
        match self.branch_of(b.control, scope) {
            Some(p) => self.exclusive_with(p, Some(other), scope),
            None => false,
        }
    }
}

#[derive(Debug)]
pub struct Assignment {
    pub node: usize,
    pub branch: Option<Branch>,
    pub references: Vec<usize>,
    pub referenced: bool,
    pub reassigned: bool,
}

#[derive(Debug)]
pub struct Reference {
    pub node: usize,
    pub branch: Option<Branch>,
}

impl Reference {
    /// Not a `zsuper` / `binding` reference.
    pub fn explicit(&self, tree: &Tree) -> bool {
        !matches!(tree.t(self.node), T::Zsuper | T::Send)
    }
}

#[derive(Debug)]
pub struct Variable {
    pub name: String,
    pub declaration: usize,
    pub scope: ScopeRef,
    pub assignments: Vec<Assignment>,
    pub references: Vec<Reference>,
    pub captured_by_block: bool,
}

impl Variable {
    pub fn is_argument(&self, tree: &Tree) -> bool {
        tree.t(self.declaration).is_argument()
    }

    pub fn is_method_argument(&self, tree: &Tree) -> bool {
        self.is_argument(tree) && matches!(tree.t(self.scope.node), T::Def | T::Defs)
    }

    /// `block_argument?`: only `block` (lambda included), not numblock / itblock.
    pub fn is_block_argument(&self, tree: &Tree) -> bool {
        self.is_argument(tree) && tree.t(self.scope.node) == T::Block
    }

    fn assign(&mut self, tree: &Tree, node: usize) {
        let branch = tree.branch_of(node, self.scope);
        if !self.captured_by_block
            && let Some(last) = self.assignments.last_mut()
            && last.branch == branch
            && !last.referenced
        {
            last.reassigned = true;
        }
        self.assignments.push(Assignment { node, branch, references: Vec::new(), referenced: false, reassigned: false });
    }

    fn reference(&mut self, tree: &Tree, node: usize) {
        let branch = tree.branch_of(node, self.scope);
        self.references.push(Reference { node, branch });
        let mut consumed: Vec<Branch> = Vec::new();
        for a in self.assignments.iter_mut().rev() {
            if a.branch.is_some_and(|b| consumed.contains(&b)) {
                continue;
            }
            let exclusive = match (a.branch, branch) {
                (Some(ab), Some(_)) => tree.exclusive_with(ab, branch, self.scope),
                _ => false,
            };
            if !exclusive {
                a.references.push(node);
                a.referenced = true;
            }
            if in_modifier_conditional(tree, a.node, node) {
                continue;
            }
            let Some(ab) = a.branch else { break };
            if Some(ab) == branch {
                break;
            }
            if !tree.main_body_of_handler(ab) {
                consumed.push(ab);
            }
        }
    }
}

fn in_modifier_conditional(tree: &Tree, assignment: usize, reference: usize) -> bool {
    let conditional = tree.ancestors(assignment).find(|&a| {
        matches!(tree.t(a), T::If | T::While | T::Until)
            && tree.nodes[a].modifier
            && tree.child(a, 0).is_some_and(|c| tree.covers(c, assignment))
    });
    let Some(c) = conditional else { return false };
    tree.covers(c, reference) && !tree.child(c, 0).is_some_and(|cond| tree.covers(cond, reference))
}

pub struct Scope {
    pub scope: ScopeRef,
    /// Variable indexes in declaration (hash key) order.
    pub variables: Vec<usize>,
}

/// Runs VariableForce over the tree, calling `after_leaving_scope` for each scope as it is left.
pub struct Force<'t, F: FnMut(&Tree, &Scope, &[Variable])> {
    tree: &'t Tree,
    scopes: Vec<Scope>,
    pub variables: Vec<Variable>,
    scanned: HashSet<usize>,
    hook: F,
}

pub fn investigate<F: FnMut(&Tree, &Scope, &[Variable])>(tree: &Tree, hook: F) {
    let Some(root) = tree.root else { return };
    let mut f = Force { tree, scopes: Vec::new(), variables: Vec::new(), scanned: HashSet::new(), hook };
    f.push_scope(root);
    f.process_node(root);
    f.pop_scope();
}

impl<F: FnMut(&Tree, &Scope, &[Variable])> Force<'_, F> {
    fn push_scope(&mut self, node: usize) {
        let naked = !self.tree.t(node).is_scope();
        self.scopes.push(Scope { scope: ScopeRef { node, naked }, variables: Vec::new() });
    }

    fn pop_scope(&mut self) {
        let scope = self.scopes.pop().unwrap();
        (self.hook)(self.tree, &scope, &self.variables);
    }

    fn find_variable(&self, name: &str) -> Option<usize> {
        for s in self.scopes.iter().rev() {
            if let Some(&v) = s.variables.iter().find(|&&v| self.variables[v].name == name) {
                return Some(v);
            }
            if !self.tree.t(s.scope.node).is_any_block() {
                return None;
            }
        }
        None
    }

    fn accessible_variables(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for s in self.scopes.iter().rev() {
            out.extend(&s.variables);
            if !self.tree.t(s.scope.node).is_any_block() {
                break;
            }
        }
        out
    }

    fn declare(&mut self, name: &str, node: usize) {
        let scope = self.scopes.last().unwrap().scope;
        self.variables.push(Variable {
            name: name.to_string(),
            declaration: node,
            scope,
            assignments: Vec::new(),
            references: Vec::new(),
            captured_by_block: false,
        });
        let v = self.variables.len() - 1;
        let vars = &mut self.scopes.last_mut().unwrap().variables;
        match vars.iter().position(|&x| self.variables[x].name == name) {
            Some(i) => vars[i] = v,
            None => vars.push(v),
        }
    }

    fn mark_captured(&mut self, v: usize) {
        let current = self.scopes.last().unwrap().scope;
        if self.tree.t(current.node).is_any_block() && self.variables[v].scope != current {
            self.variables[v].captured_by_block = true;
        }
    }

    fn assign(&mut self, name: &str, node: usize) {
        // RuboCop raises here (assigning an undeclared variable); never happens after a declare.
        let Some(v) = self.find_variable(name) else { return };
        self.mark_captured(v);
        self.variables[v].assign(self.tree, node);
    }

    fn reference(&mut self, name: &str, node: usize) {
        let Some(v) = self.find_variable(name) else { return };
        self.mark_captured(v);
        self.variables[v].reference(self.tree, node);
    }

    fn process_children(&mut self, n: usize) {
        for c in self.tree.nodes[n].children.clone().into_iter().flatten() {
            if !self.scanned.contains(&c) {
                self.process_node(c);
            }
        }
    }

    fn process(&mut self, n: Option<usize>) {
        if let Some(n) = n {
            self.process_node(n);
        }
    }

    fn process_node(&mut self, n: usize) {
        let tree = self.tree;
        let node = &tree.nodes[n];
        match node.t {
            T::Lvasgn => {
                if self.find_variable(&node.name).is_none() {
                    self.declare(&node.name, n);
                }
                self.process_children(n);
                self.assign(&node.name, n);
            }
            T::MatchWithLvasgn => {
                for name in &node.names {
                    if self.find_variable(name).is_none() {
                        self.declare(name, n);
                    }
                }
                self.process(tree.child(n, 1));
                self.process(tree.child(n, 0));
                for name in &node.names {
                    self.assign(name, n);
                }
            }
            T::MatchVar => {
                if self.find_variable(&node.name).is_none() {
                    self.declare(&node.name, n);
                }
            }
            T::Masgn => {
                self.process(tree.child(n, 1));
                self.process(tree.child(n, 0));
            }
            T::Lvar => self.reference(&node.name, n),
            T::Zsuper => {
                for v in self.accessible_variables() {
                    if self.variables[v].is_method_argument(tree) {
                        self.variables[v].reference(tree, n);
                    }
                }
                self.process_children(n);
            }
            T::Send => {
                if node.name == "binding" && node.nargs == 0 {
                    for v in self.accessible_variables() {
                        self.variables[v].reference(tree, n);
                    }
                }
                self.process_children(n);
            }
            t if t.is_argument() => {
                if !node.name.is_empty() {
                    self.declare(&node.name, n);
                }
                self.process_children(n);
            }
            T::OpAsgn | T::OrAsgn | T::AndAsgn => {
                let lhs = tree.child(n, 0).filter(|&l| tree.t(l) == T::Lvasgn);
                let Some(lhs) = lhs else { return self.process_children(n) };
                let name = &tree.nodes[lhs].name;
                if self.find_variable(name).is_none() {
                    self.declare(name, lhs);
                }
                self.reference(name, n);
                self.process(tree.last_child(n));
                self.assign(name, lhs);
            }
            T::WhilePost | T::UntilPost => {
                self.process(tree.child(n, 1));
                self.process(tree.child(n, 0));
            }
            T::For => {
                self.process(tree.child(n, 1));
                self.process(tree.child(n, 0));
                self.process(tree.child(n, 2));
            }
            t if t.is_scope() => {
                if !matches!(t, T::Def) {
                    let mut twisted = vec![tree.child(n, 0)];
                    if t == T::Class {
                        twisted.push(tree.child(n, 1));
                    }
                    for tw in twisted.into_iter().flatten() {
                        self.process_node(tw);
                        self.scanned.insert(tw);
                    }
                }
                self.push_scope(n);
                self.process_children(n);
                self.pop_scope();
            }
            _ => self.process_children(n),
        }
    }
}

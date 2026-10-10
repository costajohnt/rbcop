//! A parser-gem-shaped copy of the Prism tree, for ports that walk parser structure in depth
//! (VariableForce: child indices, `parent`, `sibling_index`, `begin`/`rescue`/`ensure` nesting).
//!
//! Only the node types those ports distinguish get their own `T`; everything else is `T::Other`
//! with its children in source order. Children are `Option<Id>`: `None` stands for a parser
//! child that is nil or not a node (a method name, an operator), so indices match parser's.
//! Built with RuboCop's builder settings: no `index`/`lambda`/`procarg0` nodes (`a[1]` is a send,
//! `-> {}` is `(block (send nil :lambda) ...)`, `|a|` is `(args (arg :a))`).

use crate::node_kind as kind;
use ruby_prism::{Node, Visit};
use std::collections::HashMap;

pub type Id = usize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum T {
    Lvasgn,
    Ivasgn,
    Cvasgn,
    Gvasgn,
    Casgn,
    Masgn,
    Mlhs,
    Splat,
    MatchWithLvasgn,
    MatchVar,
    Lvar,
    Args,
    Arg,
    Optarg,
    Restarg,
    Kwarg,
    Kwoptarg,
    Kwrestarg,
    Blockarg,
    Shadowarg,
    OpAsgn,
    OrAsgn,
    AndAsgn,
    Send,
    Csend,
    Block,
    Numblock,
    Itblock,
    Def,
    Defs,
    Class,
    Module,
    Sclass,
    If,
    Case,
    When,
    CaseMatch,
    InPattern,
    While,
    Until,
    WhilePost,
    UntilPost,
    For,
    And,
    Or,
    Rescue,
    Resbody,
    Ensure,
    Begin,
    Kwbegin,
    Array,
    Retry,
    Zsuper,
    Super,
    /// `const`; `name` is the constant name, `global` is set for a `{nil? cbase}` scope.
    Const,
    Other,
}

pub struct PNode {
    pub t: T,
    pub kids: Vec<Option<Id>>,
    pub parent: Option<Id>,
    /// Parser `source_range` (byte offsets).
    pub start: usize,
    pub end: usize,
    /// Variable, argument or method name.
    pub name: String,
    /// Named captures of a `match_with_lvasgn`, in order, unique.
    pub names: Vec<String>,
    /// `loc.name` of assignments, arguments and match variables.
    pub name_loc: Option<(usize, usize)>,
    /// `loc.operator` of assignments; `loc.keyword` of a resbody.
    pub op_loc: Option<(usize, usize)>,
    /// Modifier form of an if/unless/while/until.
    pub modifier: bool,
    /// Parser node without children (`nil`, `true`, `self`, `[]`, `()`, ...).
    pub childless: bool,
    /// A const with no scope or `::` scope.
    pub global: bool,
}

pub struct Tree {
    pub nodes: Vec<PNode>,
    pub root: Option<Id>,
}

impl Tree {
    pub fn build(program: &Node) -> Tree {
        let mut tab = Table { entries: Vec::new(), stack: Vec::new(), index: HashMap::new() };
        tab.visit(program);
        let mut conv = Conv { tab: &tab, out: Vec::new(), pattern: false };
        let root = match program.as_program_node() {
            Some(p) => conv.statements(&p.statements().as_node()),
            None => None,
        };
        Tree { nodes: conv.out, root }
    }

    pub fn t(&self, id: Id) -> T {
        self.nodes[id].t
    }

    pub fn kid(&self, id: Id, i: usize) -> Option<Id> {
        self.nodes[id].kids.get(i).copied().flatten()
    }

    pub fn parent(&self, id: Id) -> Option<Id> {
        self.nodes[id].parent
    }

    /// `node.sibling_index`.
    pub fn sibling_index(&self, id: Id) -> Option<usize> {
        let p = self.nodes[id].parent?;
        self.nodes[p].kids.iter().position(|k| *k == Some(id))
    }

    pub fn ancestors(&self, id: Id) -> impl Iterator<Item = Id> + '_ {
        std::iter::successors(self.nodes[id].parent, move |&p| self.nodes[p].parent)
    }

    pub fn child_nodes(&self, id: Id) -> impl Iterator<Item = Id> + '_ {
        self.nodes[id].kids.iter().filter_map(|k| *k)
    }

    /// `each_descendant` (pre-order, excluding the node).
    pub fn descendants(&self, id: Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut stack: Vec<Id> = self.child_nodes(id).collect();
        stack.reverse();
        while let Some(n) = stack.pop() {
            out.push(n);
            let len = stack.len();
            stack.extend(self.child_nodes(n));
            stack[len..].reverse();
        }
        out
    }

    pub fn range(&self, id: Id) -> (usize, usize) {
        (self.nodes[id].start, self.nodes[id].end)
    }

    /// `container.source_range.contains?(node.source_range)` or the same node.
    pub fn covers(&self, container: Id, node: Id) -> bool {
        container == node || (self.nodes[container].start <= self.nodes[node].start && self.nodes[node].end <= self.nodes[container].end)
    }
}

struct Entry<'pr> {
    node: Node<'pr>,
    kids: Vec<usize>,
}

/// Every Prism node with its direct children, in visit order.
struct Table<'pr> {
    entries: Vec<Entry<'pr>>,
    stack: Vec<usize>,
    index: HashMap<(usize, usize, usize), usize>,
}

fn key(n: &Node) -> (usize, usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset(), kind::of(n))
}

impl<'pr> Table<'pr> {
    fn enter(&mut self, node: Node<'pr>) {
        let i = self.entries.len();
        self.index.insert(key(&node), i);
        if let Some(&p) = self.stack.last() {
            self.entries[p].kids.push(i);
        }
        self.entries.push(Entry { node, kids: Vec::new() });
        self.stack.push(i);
    }

    /// Typed child fields are walked without `visit_branch_node_enter`: register them here.
    fn typed(&mut self, node: Node<'pr>, walk: impl FnOnce(&mut Self)) {
        let pushed = !self.stack.last().is_some_and(|&i| key(&self.entries[i].node) == key(&node));
        if pushed {
            self.enter(node);
        }
        walk(self);
        if pushed {
            self.stack.pop();
        }
    }
}

macro_rules! typed_visits {
    ($($method:ident, $walker:ident, $ty:ident;)*) => {
        $(fn $method(&mut self, node: &ruby_prism::$ty<'pr>) {
            self.typed(node.as_node(), |s| ruby_prism::$walker(s, node));
        })*
    };
}

impl<'pr> Visit<'pr> for Table<'pr> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.enter(node);
    }
    fn visit_branch_node_leave(&mut self) {
        self.stack.pop();
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.enter(node);
    }
    fn visit_leaf_node_leave(&mut self) {
        self.stack.pop();
    }
    typed_visits! {
        visit_arguments_node, visit_arguments_node, ArgumentsNode;
        visit_block_argument_node, visit_block_argument_node, BlockArgumentNode;
        visit_block_node, visit_block_node, BlockNode;
        visit_block_parameter_node, visit_block_parameter_node, BlockParameterNode;
        visit_call_node, visit_call_node, CallNode;
        visit_constant_path_node, visit_constant_path_node, ConstantPathNode;
        visit_else_node, visit_else_node, ElseNode;
        visit_ensure_node, visit_ensure_node, EnsureNode;
        visit_local_variable_target_node, visit_local_variable_target_node, LocalVariableTargetNode;
        visit_parameters_node, visit_parameters_node, ParametersNode;
        visit_rescue_node, visit_rescue_node, RescueNode;
        visit_splat_node, visit_splat_node, SplatNode;
        visit_statements_node, visit_statements_node, StatementsNode;
    }
}

struct Conv<'t, 'pr> {
    tab: &'t Table<'pr>,
    out: Vec<PNode>,
    /// Inside a pattern: local variable targets are `match_var`.
    pattern: bool,
}

fn loc(l: &ruby_prism::Location) -> (usize, usize) {
    (l.start_offset(), l.end_offset())
}

fn name_of(id: &ruby_prism::ConstantId) -> String {
    String::from_utf8_lossy(id.as_slice()).into_owned()
}

impl<'t, 'pr> Conv<'t, 'pr> {
    fn mk(&mut self, t: T, (start, end): (usize, usize), kids: Vec<Option<Id>>) -> Id {
        let id = self.out.len();
        for k in kids.iter().flatten() {
            self.out[*k].parent = Some(id);
        }
        self.out.push(PNode {
            t,
            kids,
            parent: None,
            start,
            end,
            name: String::new(),
            names: Vec::new(),
            name_loc: None,
            op_loc: None,
            modifier: false,
            childless: false,
            global: false,
        });
        id
    }

    fn named(&mut self, t: T, range: (usize, usize), kids: Vec<Option<Id>>, name: String, name_loc: Option<(usize, usize)>) -> Id {
        let id = self.mk(t, range, kids);
        self.out[id].name = name;
        self.out[id].name_loc = name_loc;
        id
    }

    fn entry(&self, n: &Node) -> Option<usize> {
        self.tab.index.get(&key(n)).copied()
    }

    /// The parser nodes for a Prism node (several for a transparent wrapper).
    fn conv(&mut self, n: &Node) -> Vec<Id> {
        match self.entry(n) {
            Some(i) => self.conv_entry(i),
            None => Vec::new(),
        }
    }

    /// A single parser node (several statements become a `begin`).
    fn one(&mut self, n: &Node) -> Option<Id> {
        let v = self.conv(n);
        self.wrap(v)
    }

    fn opt(&mut self, n: Option<Node>) -> Option<Id> {
        n.and_then(|n| self.one(&n))
    }

    fn wrap(&mut self, v: Vec<Id>) -> Option<Id> {
        match v.len() {
            0 => None,
            1 => Some(v[0]),
            _ => {
                let range = (self.out[v[0]].start, self.out[*v.last().unwrap()].end);
                Some(self.mk(T::Begin, range, v.into_iter().map(Some).collect()))
            }
        }
    }

    /// A statements body: nil, the statement, or `(begin ...)`.
    fn statements(&mut self, n: &Node) -> Option<Id> {
        let v = self.kids_of(n);
        self.wrap(v)
    }

    fn stmts(&mut self, s: Option<ruby_prism::StatementsNode>) -> Option<Id> {
        s.and_then(|s| self.statements(&s.as_node()))
    }

    /// Body of a def/class/block: statements, or an implicit begin with rescue/ensure.
    fn body(&mut self, n: Option<Node>) -> Option<Id> {
        let n = n?;
        if let Some(b) = n.as_begin_node().filter(|b| b.begin_keyword_loc().is_none()) {
            return self.begin_struct(&b);
        }
        if n.as_statements_node().is_some() {
            return self.statements(&n);
        }
        self.one(&n)
    }

    /// Converted direct children of a Prism node, in visit order.
    fn kids_of(&mut self, n: &Node) -> Vec<Id> {
        let Some(i) = self.entry(n) else { return Vec::new() };
        let tab = self.tab;
        tab.entries[i].kids.iter().flat_map(|&k| self.conv_entry(k)).collect()
    }

    fn generic(&mut self, t: T, n: &Node) -> Id {
        let kids = self.kids_of(n);
        self.mk(t, loc(&n.location()), kids.into_iter().map(Some).collect())
    }

    fn rescue_chain(&mut self, r: Option<ruby_prism::RescueNode>) -> Vec<Id> {
        let mut out = Vec::new();
        let mut cur = r;
        while let Some(r) = cur {
            let excs: Vec<Node> = r.exceptions().iter().collect();
            let list = if excs.is_empty() {
                None
            } else {
                let range = (excs[0].location().start_offset(), excs.last().unwrap().location().end_offset());
                let kids = excs.iter().flat_map(|e| self.conv(e)).map(Some).collect();
                Some(self.mk(T::Array, range, kids))
            };
            let var = self.opt(r.reference());
            let body = self.stmts(r.statements());
            let kw = loc(&r.keyword_loc());
            let end = [body, var, list].iter().flatten().map(|&k| self.out[k].end).max().unwrap_or(kw.1);
            let id = self.mk(T::Resbody, (kw.0, end), vec![list, var, body]);
            self.out[id].op_loc = Some(kw);
            out.push(id);
            cur = r.subsequent();
        }
        out
    }

    /// The rescue/ensure structure of a BeginNode: `(ensure (rescue body resbody... else) ensure_body)`.
    fn begin_struct(&mut self, b: &ruby_prism::BeginNode) -> Option<Id> {
        let mut base = self.stmts(b.statements());
        if b.rescue_clause().is_some() {
            let resbodies = self.rescue_chain(b.rescue_clause());
            let els = b.else_clause().and_then(|e| self.stmts(e.statements()));
            let start = base.map_or(self.out[resbodies[0]].start, |x| self.out[x].start);
            let end = els.map_or(self.out[*resbodies.last().unwrap()].end, |x| self.out[x].end);
            let mut kids = vec![base];
            kids.extend(resbodies.into_iter().map(Some));
            kids.push(els);
            base = Some(self.mk(T::Rescue, (start, end), kids));
        }
        if let Some(e) = b.ensure_clause() {
            let body = self.stmts(e.statements());
            let kw = loc(&e.ensure_keyword_loc());
            let start = base.map_or(kw.0, |x| self.out[x].start);
            let end = body.map_or(kw.1, |x| self.out[x].end);
            base = Some(self.mk(T::Ensure, (start, end), vec![base, body]));
        }
        base
    }

    fn param(&mut self, n: &Node) -> Option<Id> {
        let range = loc(&n.location());
        let k = kind::of(n);
        Some(match k {
            kind::REQUIRED_PARAMETER_NODE => {
                let p = n.as_required_parameter_node().unwrap();
                self.named(T::Arg, range, vec![None], name_of(&p.name()), Some(range))
            }
            kind::OPTIONAL_PARAMETER_NODE => {
                let p = n.as_optional_parameter_node().unwrap();
                let v = self.one(&p.value());
                self.named(T::Optarg, range, vec![None, v], name_of(&p.name()), Some(loc(&p.name_loc())))
            }
            kind::REST_PARAMETER_NODE => {
                let p = n.as_rest_parameter_node().unwrap();
                let name = p.name().map(|c| name_of(&c)).unwrap_or_default();
                self.named(T::Restarg, range, vec![None], name, p.name_loc().map(|l| loc(&l)))
            }
            kind::REQUIRED_KEYWORD_PARAMETER_NODE => {
                let p = n.as_required_keyword_parameter_node().unwrap();
                let (s, e) = loc(&p.name_loc());
                self.named(T::Kwarg, range, vec![None], name_of(&p.name()), Some((s, e - 1)))
            }
            kind::OPTIONAL_KEYWORD_PARAMETER_NODE => {
                let p = n.as_optional_keyword_parameter_node().unwrap();
                let (s, e) = loc(&p.name_loc());
                let v = self.one(&p.value());
                self.named(T::Kwoptarg, range, vec![None, v], name_of(&p.name()), Some((s, e - 1)))
            }
            kind::KEYWORD_REST_PARAMETER_NODE => {
                let p = n.as_keyword_rest_parameter_node().unwrap();
                let name = p.name().map(|c| name_of(&c)).unwrap_or_default();
                self.named(T::Kwrestarg, range, vec![None], name, p.name_loc().map(|l| loc(&l)))
            }
            kind::BLOCK_PARAMETER_NODE => {
                let p = n.as_block_parameter_node().unwrap();
                let name = p.name().map(|c| name_of(&c)).unwrap_or_default();
                self.named(T::Blockarg, range, vec![None], name, p.name_loc().map(|l| loc(&l)))
            }
            kind::BLOCK_LOCAL_VARIABLE_NODE => {
                let p = n.as_block_local_variable_node().unwrap();
                self.named(T::Shadowarg, range, vec![None], name_of(&p.name()), Some(range))
            }
            kind::MULTI_TARGET_NODE => {
                let m = n.as_multi_target_node().unwrap();
                let mut kids = Vec::new();
                for l in m.lefts().iter() {
                    kids.push(self.param(&l));
                }
                if let Some(r) = m.rest() {
                    if let Some(s) = r.as_splat_node() {
                        let (name, nl) = match s.expression() {
                            Some(e) => (
                                e.as_required_parameter_node().map(|p| name_of(&p.name())).unwrap_or_default(),
                                Some(loc(&e.location())),
                            ),
                            None => (String::new(), None),
                        };
                        kids.push(Some(self.named(T::Restarg, loc(&r.location()), vec![None], name, nl)));
                    }
                }
                for l in m.rights().iter() {
                    kids.push(self.param(&l));
                }
                self.mk(T::Mlhs, range, kids)
            }
            kind::IMPLICIT_REST_NODE => return None,
            _ => self.generic(T::Other, n),
        })
    }

    fn params(&mut self, p: Option<ruby_prism::ParametersNode>, range: (usize, usize)) -> Id {
        let mut kids = Vec::new();
        let r = p.as_ref().map_or(range, |p| loc(&p.location()));
        if let Some(p) = p {
            let mut list: Vec<Node> = p.requireds().iter().collect();
            list.extend(p.optionals().iter());
            list.extend(p.rest());
            list.extend(p.posts().iter());
            list.extend(p.keywords().iter());
            list.extend(p.keyword_rest());
            list.extend(p.block().map(|b| b.as_node()));
            for n in &list {
                kids.push(self.param(n));
            }
        }
        self.mk(T::Args, r, kids.into_iter().filter(|k| k.is_some()).collect())
    }

    /// Block type and args node for a block/lambda's parameters.
    fn block_params(&mut self, params: Option<Node>, at: (usize, usize)) -> (T, Option<Id>) {
        match params {
            None => (T::Block, Some(self.mk(T::Args, at, vec![]))),
            Some(p) => match kind::of(&p) {
                kind::NUMBERED_PARAMETERS_NODE => (T::Numblock, None),
                kind::IT_PARAMETERS_NODE => (T::Itblock, None),
                _ => {
                    let bp = p.as_block_parameters_node().unwrap();
                    let args = self.params(bp.parameters(), loc(&p.location()));
                    let locals: Vec<Node> = bp.locals().iter().collect();
                    for l in &locals {
                        if let Some(s) = self.param(l) {
                            self.out[s].parent = Some(args);
                            self.out[args].kids.push(Some(s));
                        }
                    }
                    self.out[args].start = loc(&p.location()).0;
                    self.out[args].end = loc(&p.location()).1;
                    (T::Block, Some(args))
                }
            },
        }
    }

    fn block(&mut self, send: Id, b: &ruby_prism::BlockNode, range: (usize, usize)) -> Id {
        let (t, args) = self.block_params(b.parameters(), loc(&b.opening_loc()));
        let body = self.body(b.body());
        self.mk(t, range, vec![Some(send), args, body])
    }

    fn call(&mut self, c: &ruby_prism::CallNode) -> Id {
        let recv = self.opt(c.receiver());
        let mut kids = vec![recv, None];
        if let Some(a) = c.arguments() {
            for arg in a.arguments().iter() {
                kids.extend(self.conv(&arg).into_iter().map(Some));
            }
        }
        let literal = c.block().and_then(|b| b.as_block_node());
        if let Some(b) = c.block().filter(|b| b.as_block_node().is_none()) {
            kids.push(self.one(&b));
        }
        let t = if crate::ast::is_csend(c) { T::Csend } else { T::Send };
        let send = self.mk(t, crate::ast::send_range(c), kids);
        self.out[send].name = crate::ast::method_name(c).to_string();
        match literal {
            Some(b) => self.block(send, &b, loc(&c.location())),
            None => send,
        }
    }

    /// `x op= v`, `x ||= v`, `x &&= v` for any target: the target node is built from the Prism
    /// node's other children.
    fn op_assign(&mut self, n: &Node, k: usize) -> Id {
        use kind::*;
        let (lhs_t, op_t) = match k {
            LOCAL_VARIABLE_OPERATOR_WRITE_NODE => (T::Lvasgn, T::OpAsgn),
            LOCAL_VARIABLE_OR_WRITE_NODE => (T::Lvasgn, T::OrAsgn),
            LOCAL_VARIABLE_AND_WRITE_NODE => (T::Lvasgn, T::AndAsgn),
            INSTANCE_VARIABLE_OPERATOR_WRITE_NODE => (T::Ivasgn, T::OpAsgn),
            INSTANCE_VARIABLE_OR_WRITE_NODE => (T::Ivasgn, T::OrAsgn),
            INSTANCE_VARIABLE_AND_WRITE_NODE => (T::Ivasgn, T::AndAsgn),
            CLASS_VARIABLE_OPERATOR_WRITE_NODE => (T::Cvasgn, T::OpAsgn),
            CLASS_VARIABLE_OR_WRITE_NODE => (T::Cvasgn, T::OrAsgn),
            CLASS_VARIABLE_AND_WRITE_NODE => (T::Cvasgn, T::AndAsgn),
            GLOBAL_VARIABLE_OPERATOR_WRITE_NODE => (T::Gvasgn, T::OpAsgn),
            GLOBAL_VARIABLE_OR_WRITE_NODE => (T::Gvasgn, T::OrAsgn),
            GLOBAL_VARIABLE_AND_WRITE_NODE => (T::Gvasgn, T::AndAsgn),
            CONSTANT_OPERATOR_WRITE_NODE | CONSTANT_PATH_OPERATOR_WRITE_NODE => (T::Casgn, T::OpAsgn),
            CONSTANT_OR_WRITE_NODE | CONSTANT_PATH_OR_WRITE_NODE => (T::Casgn, T::OrAsgn),
            CONSTANT_AND_WRITE_NODE | CONSTANT_PATH_AND_WRITE_NODE => (T::Casgn, T::AndAsgn),
            CALL_OPERATOR_WRITE_NODE | INDEX_OPERATOR_WRITE_NODE => (T::Send, T::OpAsgn),
            CALL_OR_WRITE_NODE | INDEX_OR_WRITE_NODE => (T::Send, T::OrAsgn),
            _ => (T::Send, T::AndAsgn),
        };
        let mut kids = self.kids_of(n);
        let value = kids.pop();
        let range = loc(&n.location());
        let (name, name_loc, op_loc) = if let Some(w) = n.as_local_variable_operator_write_node() {
            (name_of(&w.name()), loc(&w.name_loc()), loc(&w.binary_operator_loc()))
        } else if let Some(w) = n.as_local_variable_or_write_node() {
            (name_of(&w.name()), loc(&w.name_loc()), loc(&w.operator_loc()))
        } else if let Some(w) = n.as_local_variable_and_write_node() {
            (name_of(&w.name()), loc(&w.name_loc()), loc(&w.operator_loc()))
        } else {
            (String::new(), (range.0, kids.last().map_or(range.0, |&k| self.out[k].end)), (0, 0))
        };
        let mut lhs_kids: Vec<Option<Id>> = match lhs_t {
            T::Send => {
                let mut v = vec![kids.first().copied(), None];
                v.extend(kids.iter().skip(1).map(|&k| Some(k)));
                v
            }
            T::Casgn => kids.into_iter().map(Some).collect(),
            _ => vec![None],
        };
        if lhs_t == T::Send && lhs_kids[0].is_none() {
            lhs_kids.truncate(2);
        }
        let lhs = self.named(lhs_t, name_loc, lhs_kids, name, Some(name_loc));
        let kids = if op_t == T::OpAsgn { vec![Some(lhs), None, value] } else { vec![Some(lhs), value] };
        let id = self.mk(op_t, range, kids);
        self.out[id].op_loc = (op_loc != (0, 0)).then_some(op_loc);
        id
    }

    fn conv_entry(&mut self, i: usize) -> Vec<Id> {
        let tab = self.tab;
        let n = &tab.entries[i].node;
        let range = loc(&n.location());
        let k = kind::of(n);
        use kind::*;
        let id = match k {
            STATEMENTS_NODE | ARGUMENTS_NODE | ELSE_NODE | IMPLICIT_NODE | EMBEDDED_STATEMENTS_NODE => {
                return self.kids_of(n);
            }
            IMPLICIT_REST_NODE => return Vec::new(),
            CALL_NODE => self.call(&n.as_call_node().unwrap()),
            LAMBDA_NODE => {
                let l = n.as_lambda_node().unwrap();
                let send = self.named(T::Send, loc(&l.operator_loc()), vec![None, None], "lambda".into(), None);
                let (t, args) = self.block_params(l.parameters(), loc(&l.opening_loc()));
                let body = self.body(l.body());
                self.mk(t, range, vec![Some(send), args, body])
            }
            FORWARDING_SUPER_NODE => {
                let s = n.as_forwarding_super_node().unwrap();
                match s.block() {
                    Some(b) => {
                        let kw = (range.0, range.0 + 5);
                        let z = self.mk(T::Zsuper, kw, vec![]);
                        self.out[z].childless = true;
                        self.block(z, &b, range)
                    }
                    None => {
                        let z = self.mk(T::Zsuper, range, vec![]);
                        self.out[z].childless = true;
                        z
                    }
                }
            }
            SUPER_NODE => {
                let s = n.as_super_node().unwrap();
                let mut kids = Vec::new();
                if let Some(a) = s.arguments() {
                    for arg in a.arguments().iter() {
                        kids.extend(self.conv(&arg).into_iter().map(Some));
                    }
                }
                let literal = s.block().and_then(|b| b.as_block_node());
                if let Some(b) = s.block().filter(|b| b.as_block_node().is_none()) {
                    kids.push(self.one(&b));
                }
                match literal {
                    Some(b) => {
                        let end = s.rparen_loc().map_or_else(
                            || kids.iter().flatten().map(|&k| self.out[k].end).max().unwrap_or(range.0 + 5),
                            |l| l.end_offset(),
                        );
                        let sup = self.mk(T::Super, (range.0, end), kids);
                        self.block(sup, &b, range)
                    }
                    None => self.mk(T::Super, range, kids),
                }
            }
            PROGRAM_NODE => return self.kids_of(n),
            PARENTHESES_NODE => {
                let kids = self.kids_of(n);
                let id = self.mk(T::Begin, range, kids.into_iter().map(Some).collect());
                self.out[id].childless = self.out[id].kids.is_empty();
                id
            }
            BEGIN_NODE => {
                let b = n.as_begin_node().unwrap();
                if b.begin_keyword_loc().is_some() {
                    let kids = if b.rescue_clause().is_some() || b.ensure_clause().is_some() {
                        vec![self.begin_struct(&b)]
                    } else {
                        let v = b.statements().map(|s| self.kids_of(&s.as_node())).unwrap_or_default();
                        v.into_iter().map(Some).collect()
                    };
                    self.mk(T::Kwbegin, range, kids)
                } else {
                    return self.begin_struct(&b).into_iter().collect();
                }
            }
            RESCUE_MODIFIER_NODE => {
                let r = n.as_rescue_modifier_node().unwrap();
                let body = self.one(&r.expression());
                let res = self.one(&r.rescue_expression());
                let kw = loc(&r.keyword_loc());
                let rb = self.mk(T::Resbody, (kw.0, range.1), vec![None, None, res]);
                self.out[rb].op_loc = Some(kw);
                self.mk(T::Rescue, range, vec![body, Some(rb), None])
            }
            IF_NODE => {
                let f = n.as_if_node().unwrap();
                let pred = self.one(&f.predicate());
                let then = self.stmts(f.statements());
                let els = match f.subsequent() {
                    Some(s) => match s.as_else_node() {
                        Some(e) => self.stmts(e.statements()),
                        None => self.one(&s),
                    },
                    None => None,
                };
                let id = self.mk(T::If, range, vec![pred, then, els]);
                self.out[id].modifier =
                    f.if_keyword_loc().is_some_and(|l| l.as_slice() == b"if") && f.end_keyword_loc().is_none();
                id
            }
            UNLESS_NODE => {
                let f = n.as_unless_node().unwrap();
                let pred = self.one(&f.predicate());
                let then = self.stmts(f.statements());
                let els = f.else_clause().and_then(|e| self.stmts(e.statements()));
                let id = self.mk(T::If, range, vec![pred, els, then]);
                self.out[id].modifier = f.end_keyword_loc().is_none();
                id
            }
            CASE_NODE => {
                let c = n.as_case_node().unwrap();
                let mut kids = vec![self.opt(c.predicate())];
                for w in c.conditions().iter() {
                    kids.push(self.one(&w));
                }
                kids.push(c.else_clause().and_then(|e| self.stmts(e.statements())));
                self.mk(T::Case, range, kids)
            }
            WHEN_NODE => {
                let w = n.as_when_node().unwrap();
                let mut kids = Vec::new();
                for c in w.conditions().iter() {
                    kids.extend(self.conv(&c).into_iter().map(Some));
                }
                kids.push(self.stmts(w.statements()));
                self.mk(T::When, range, kids)
            }
            CASE_MATCH_NODE => {
                let c = n.as_case_match_node().unwrap();
                let mut kids = vec![self.opt(c.predicate())];
                for w in c.conditions().iter() {
                    kids.push(self.one(&w));
                }
                kids.push(c.else_clause().and_then(|e| self.stmts(e.statements())));
                self.mk(T::CaseMatch, range, kids)
            }
            IN_NODE => {
                let p = n.as_in_node().unwrap();
                let pat = p.pattern();
                let (pattern, guard) = match (pat.as_if_node(), pat.as_unless_node()) {
                    (Some(g), _) => (g.statements().and_then(|s| s.body().iter().next()), Some(g.predicate())),
                    (_, Some(g)) => (g.statements().and_then(|s| s.body().iter().next()), Some(g.predicate())),
                    _ => (Some(pat), None),
                };
                let saved = std::mem::replace(&mut self.pattern, true);
                let pattern = self.opt(pattern);
                self.pattern = saved;
                let guard = guard.map(|g| {
                    let c = self.one(&g);
                    let r = loc(&g.location());
                    self.mk(T::Other, r, vec![c])
                });
                let body = self.stmts(p.statements());
                self.mk(T::InPattern, range, vec![pattern, guard, body])
            }
            MATCH_PREDICATE_NODE | MATCH_REQUIRED_NODE => {
                let (value, pat) = match n.as_match_predicate_node() {
                    Some(m) => (m.value(), m.pattern()),
                    None => {
                        let m = n.as_match_required_node().unwrap();
                        (m.value(), m.pattern())
                    }
                };
                let v = self.one(&value);
                let saved = std::mem::replace(&mut self.pattern, true);
                let p = self.one(&pat);
                self.pattern = saved;
                self.mk(T::Other, range, vec![v, p])
            }
            WHILE_NODE | UNTIL_NODE => {
                let (pred, stmts, closing, post) = match n.as_while_node() {
                    Some(w) => (w.predicate(), w.statements(), w.closing_loc().is_some(), w.is_begin_modifier()),
                    None => {
                        let w = n.as_until_node().unwrap();
                        (w.predicate(), w.statements(), w.closing_loc().is_some(), w.is_begin_modifier())
                    }
                };
                let t = match (k == WHILE_NODE, post) {
                    (true, false) => T::While,
                    (true, true) => T::WhilePost,
                    (false, false) => T::Until,
                    (false, true) => T::UntilPost,
                };
                let c = self.one(&pred);
                let body = self.stmts(stmts);
                let id = self.mk(t, range, vec![c, body]);
                self.out[id].modifier = !closing && !post;
                id
            }
            FOR_NODE => {
                let f = n.as_for_node().unwrap();
                let index = self.one(&f.index());
                let coll = self.one(&f.collection());
                let body = self.stmts(f.statements());
                self.mk(T::For, range, vec![index, coll, body])
            }
            AND_NODE => self.generic(T::And, n),
            OR_NODE => self.generic(T::Or, n),
            LOCAL_VARIABLE_WRITE_NODE => {
                let w = n.as_local_variable_write_node().unwrap();
                let v = self.one(&w.value());
                let id = self.named(T::Lvasgn, range, vec![None, v], name_of(&w.name()), Some(loc(&w.name_loc())));
                self.out[id].op_loc = Some(loc(&w.operator_loc()));
                id
            }
            LOCAL_VARIABLE_TARGET_NODE => {
                let w = n.as_local_variable_target_node().unwrap();
                let name = name_of(&w.name());
                if self.pattern {
                    let nl = (range.0, range.0 + name.len());
                    self.named(T::MatchVar, range, vec![None], name, Some(nl))
                } else {
                    self.named(T::Lvasgn, range, vec![None], name, Some(range))
                }
            }
            LOCAL_VARIABLE_READ_NODE => {
                let r = n.as_local_variable_read_node().unwrap();
                self.named(T::Lvar, range, vec![None], name_of(&r.name()), None)
            }
            INSTANCE_VARIABLE_WRITE_NODE | INSTANCE_VARIABLE_TARGET_NODE => self.asgn(T::Ivasgn, n),
            CLASS_VARIABLE_WRITE_NODE | CLASS_VARIABLE_TARGET_NODE => self.asgn(T::Cvasgn, n),
            GLOBAL_VARIABLE_WRITE_NODE | GLOBAL_VARIABLE_TARGET_NODE => self.asgn(T::Gvasgn, n),
            CONSTANT_WRITE_NODE | CONSTANT_TARGET_NODE | CONSTANT_PATH_WRITE_NODE | CONSTANT_PATH_TARGET_NODE => {
                self.generic(T::Casgn, n)
            }
            LOCAL_VARIABLE_OPERATOR_WRITE_NODE
            | LOCAL_VARIABLE_OR_WRITE_NODE
            | LOCAL_VARIABLE_AND_WRITE_NODE
            | INSTANCE_VARIABLE_OPERATOR_WRITE_NODE
            | INSTANCE_VARIABLE_OR_WRITE_NODE
            | INSTANCE_VARIABLE_AND_WRITE_NODE
            | CLASS_VARIABLE_OPERATOR_WRITE_NODE
            | CLASS_VARIABLE_OR_WRITE_NODE
            | CLASS_VARIABLE_AND_WRITE_NODE
            | GLOBAL_VARIABLE_OPERATOR_WRITE_NODE
            | GLOBAL_VARIABLE_OR_WRITE_NODE
            | GLOBAL_VARIABLE_AND_WRITE_NODE
            | CONSTANT_OPERATOR_WRITE_NODE
            | CONSTANT_OR_WRITE_NODE
            | CONSTANT_AND_WRITE_NODE
            | CONSTANT_PATH_OPERATOR_WRITE_NODE
            | CONSTANT_PATH_OR_WRITE_NODE
            | CONSTANT_PATH_AND_WRITE_NODE
            | CALL_OPERATOR_WRITE_NODE
            | CALL_OR_WRITE_NODE
            | CALL_AND_WRITE_NODE
            | INDEX_OPERATOR_WRITE_NODE
            | INDEX_OR_WRITE_NODE
            | INDEX_AND_WRITE_NODE => self.op_assign(n, k),
            CALL_TARGET_NODE | INDEX_TARGET_NODE => {
                let kids = self.kids_of(n);
                let mut v = vec![kids.first().copied(), None];
                v.extend(kids.iter().skip(1).map(|&k| Some(k)));
                self.mk(T::Send, range, v)
            }
            MULTI_WRITE_NODE => {
                let m = n.as_multi_write_node().unwrap();
                let mut kids = Vec::new();
                for l in m.lefts().iter() {
                    kids.extend(self.conv(&l).into_iter().map(Some));
                }
                if let Some(r) = m.rest() {
                    kids.extend(self.conv(&r).into_iter().map(Some));
                }
                for l in m.rights().iter() {
                    kids.extend(self.conv(&l).into_iter().map(Some));
                }
                let start = m.lparen_loc().map_or(range.0, |l| l.start_offset());
                let end = m.rparen_loc().map_or_else(
                    || kids.iter().flatten().map(|&k| self.out[k].end).max().unwrap_or(start),
                    |l| l.end_offset(),
                );
                let mlhs = self.mk(T::Mlhs, (start, end), kids);
                let v = self.one(&m.value());
                self.mk(T::Masgn, range, vec![Some(mlhs), v])
            }
            MULTI_TARGET_NODE => self.generic(T::Mlhs, n),
            SPLAT_NODE => self.generic(T::Splat, n),
            MATCH_WRITE_NODE => {
                let m = n.as_match_write_node().unwrap();
                let call = m.call();
                let re = self.opt(call.receiver());
                let rhs = call.arguments().and_then(|a| a.arguments().iter().next()).and_then(|a| self.one(&a));
                let id = self.mk(T::MatchWithLvasgn, range, vec![re, rhs]);
                let mut names: Vec<String> = Vec::new();
                for t in m.targets().iter() {
                    if let Some(t) = t.as_local_variable_target_node() {
                        let name = name_of(&t.name());
                        if !names.contains(&name) {
                            names.push(name);
                        }
                    }
                }
                self.out[id].names = names;
                id
            }
            DEF_NODE => {
                let d = n.as_def_node().unwrap();
                let recv = self.opt(d.receiver());
                let args = self.params(d.parameters(), loc(&d.name_loc()));
                let body = self.body(d.body());
                let id = if d.receiver().is_some() {
                    self.mk(T::Defs, range, vec![recv, None, Some(args), body])
                } else {
                    self.mk(T::Def, range, vec![None, Some(args), body])
                };
                self.out[id].name = name_of(&d.name());
                id
            }
            CLASS_NODE => {
                let c = n.as_class_node().unwrap();
                let name = self.one(&c.constant_path());
                let sup = self.opt(c.superclass());
                let body = self.body(c.body());
                self.mk(T::Class, range, vec![name, sup, body])
            }
            MODULE_NODE => {
                let c = n.as_module_node().unwrap();
                let name = self.one(&c.constant_path());
                let body = self.body(c.body());
                self.mk(T::Module, range, vec![name, body])
            }
            SINGLETON_CLASS_NODE => {
                let c = n.as_singleton_class_node().unwrap();
                let e = self.one(&c.expression());
                let body = self.body(c.body());
                self.mk(T::Sclass, range, vec![e, body])
            }
            RETRY_NODE => self.mk(T::Retry, range, vec![]),
            CONSTANT_READ_NODE => {
                let c = n.as_constant_read_node().unwrap();
                let id = self.named(T::Const, range, vec![None, None], name_of(&c.name()), None);
                self.out[id].global = true;
                id
            }
            CONSTANT_PATH_NODE => {
                let c = n.as_constant_path_node().unwrap();
                let scope = self.opt(c.parent());
                let name = c.name().map(|n| name_of(&n)).unwrap_or_default();
                let id = self.named(T::Const, range, vec![scope, None], name, None);
                self.out[id].global = c.parent().is_none();
                id
            }
            ARRAY_NODE => {
                let id = self.generic(T::Array, n);
                self.out[id].childless = self.out[id].kids.is_empty();
                id
            }
            HASH_NODE => {
                let id = self.generic(T::Other, n);
                self.out[id].childless = self.out[id].kids.is_empty();
                id
            }
            NIL_NODE | TRUE_NODE | FALSE_NODE | SELF_NODE | REDO_NODE => {
                let id = self.mk(T::Other, range, vec![]);
                self.out[id].childless = true;
                id
            }
            _ => self.generic(T::Other, n),
        };
        vec![id]
    }

    fn asgn(&mut self, t: T, n: &Node) -> Id {
        let kids = self.kids_of(n);
        let mut v = vec![None];
        v.extend(kids.into_iter().map(Some));
        self.mk(t, loc(&n.location()), v)
    }
}

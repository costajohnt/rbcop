//! Port of Lint::Utils::NilReceiverChecker: can the receiver of a `&.` call be nil, judging by
//! the code that runs before it? It walks parser-gem parents and left siblings, so it works on a
//! small parser-shaped tree built from the Prism AST.

use crate::ast::{method_name, send_range};
use crate::source::Source;
use ruby_prism::{CallNode, Node, Visit};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    Send,
    Csend,
    Block,
    Begin,
    Kwbegin,
    If,
    Case,
    When,
    And,
    Or,
    Pair,
    Asgn,
    Scope,
    Resbody,
    Rescue,
    Ensure,
    Lvar,
    Other,
}

#[derive(Debug)]
struct PNode {
    ty: Ty,
    start: usize,
    end: usize,
    parent: Option<usize>,
    children: Vec<usize>,
    /// send: method name; lvar: variable name; if: keyword (`if`/`unless`/`elsif`/``).
    name: String,
    /// send: receiver; if/case: condition; and/or: lhs; asgn: expression.
    first: Option<usize>,
    /// send: arguments (incl. block_pass); when: conditions.
    list: Vec<usize>,
    /// if: else branch; block: body; ensure: ensure body (`branch`).
    second: Option<usize>,
    /// block: names in `argument_list`, and whether `arguments` is non-empty.
    arg_names: Vec<String>,
    has_args: bool,
}

struct Tree {
    nodes: Vec<PNode>,
}

fn prism_children<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    struct C<'pr> {
        depth: usize,
        out: Vec<Node<'pr>>,
    }
    impl<'pr> Visit<'pr> for C<'pr> {
        fn visit_branch_node_enter(&mut self, n: Node<'pr>) {
            if self.depth == 1 {
                self.out.push(n);
            }
            self.depth += 1;
        }
        fn visit_branch_node_leave(&mut self) {
            self.depth -= 1;
        }
        fn visit_leaf_node_enter(&mut self, n: Node<'pr>) {
            if self.depth == 1 {
                self.out.push(n);
            }
            self.depth += 1;
        }
        fn visit_leaf_node_leave(&mut self) {
            self.depth -= 1;
        }
    }
    let mut c = C { depth: 0, out: Vec::new() };
    c.visit(node);
    c.out
}

fn param_names(node: &Node) -> Vec<String> {
    #[derive(Default)]
    struct P {
        names: Vec<String>,
    }
    impl<'pr> Visit<'pr> for P {
        fn visit_leaf_node_enter(&mut self, n: Node<'pr>) {
            self.add(&n);
        }
        fn visit_branch_node_enter(&mut self, n: Node<'pr>) {
            self.add(&n);
        }
    }
    impl P {
        fn add(&mut self, n: &Node) {
            let name = match n {
                Node::RequiredParameterNode { .. } => Some(n.as_required_parameter_node().unwrap().name()),
                Node::OptionalParameterNode { .. } => Some(n.as_optional_parameter_node().unwrap().name()),
                Node::RestParameterNode { .. } => n.as_rest_parameter_node().unwrap().name(),
                Node::RequiredKeywordParameterNode { .. } => Some(n.as_required_keyword_parameter_node().unwrap().name()),
                Node::OptionalKeywordParameterNode { .. } => Some(n.as_optional_keyword_parameter_node().unwrap().name()),
                Node::KeywordRestParameterNode { .. } => n.as_keyword_rest_parameter_node().unwrap().name(),
                Node::BlockParameterNode { .. } => n.as_block_parameter_node().unwrap().name(),
                Node::BlockLocalVariableNode { .. } => Some(n.as_block_local_variable_node().unwrap().name()),
                _ => None,
            };
            if let Some(name) = name {
                self.names.push(String::from_utf8_lossy(name.as_slice()).into_owned());
            }
        }
    }
    let mut p = P::default();
    p.visit(node);
    p.names
}

impl Tree {
    fn push(&mut self, ty: Ty, start: usize, end: usize, parent: Option<usize>) -> usize {
        self.nodes.push(PNode {
            ty,
            start,
            end,
            parent,
            children: Vec::new(),
            name: String::new(),
            first: None,
            list: Vec::new(),
            second: None,
            arg_names: Vec::new(),
            has_args: false,
        });
        self.nodes.len() - 1
    }

    fn span(node: &Node) -> (usize, usize) {
        let l = node.location();
        (l.start_offset(), l.end_offset())
    }

    /// A body: one statement is itself, several are a `begin`.
    fn body(&mut self, node: Option<Node>, parent: Option<usize>) -> Option<usize> {
        let node = node?;
        let Some(s) = node.as_statements_node() else { return self.conv(&node, parent) };
        let stmts: Vec<Node> = s.body().iter().collect();
        match stmts.len() {
            0 => None,
            1 => self.conv(&stmts[0], parent),
            _ => {
                let (a, b) = Self::span(&node);
                let i = self.push(Ty::Begin, a, b, parent);
                let children = stmts.iter().filter_map(|c| self.conv(c, Some(i))).collect();
                self.nodes[i].children = children;
                Some(i)
            }
        }
    }

    /// Generic children: statements become a body, `ArgumentsNode`/`ElseNode` are flattened.
    fn flatten(&mut self, children: Vec<Node>, parent: usize) -> Vec<usize> {
        let mut out = Vec::new();
        for c in children {
            match &c {
                Node::StatementsNode { .. } => out.extend(self.body(Some(c), Some(parent))),
                Node::ArgumentsNode { .. } => {
                    for a in c.as_arguments_node().unwrap().arguments().iter() {
                        out.extend(self.conv(&a, Some(parent)));
                    }
                }
                Node::ElseNode { .. } => {
                    let s = c.as_else_node().unwrap().statements().map(|s| s.as_node());
                    out.extend(self.body(s, Some(parent)));
                }
                _ => out.extend(self.conv(&c, Some(parent))),
            }
        }
        out
    }

    fn generic(&mut self, ty: Ty, node: &Node, parent: Option<usize>) -> usize {
        let (a, b) = Self::span(node);
        let i = self.push(ty, a, b, parent);
        let children = self.flatten(prism_children(node), i);
        self.nodes[i].children = children;
        i
    }

    fn conv(&mut self, node: &Node, parent: Option<usize>) -> Option<usize> {
        let (start, end) = Self::span(node);
        Some(match node {
            Node::ProgramNode { .. } => {
                return self.body(Some(node.as_program_node().unwrap().statements().as_node()), parent);
            }
            Node::StatementsNode { .. } => return self.body(Some(node.as_statements_node().unwrap().as_node()), parent),
            Node::ParenthesesNode { .. } | Node::EmbeddedStatementsNode { .. } => {
                let i = self.push(Ty::Begin, start, end, parent);
                let body = match node {
                    Node::ParenthesesNode { .. } => node.as_parentheses_node().unwrap().body(),
                    _ => node.as_embedded_statements_node().unwrap().statements().map(|s| s.as_node()),
                };
                let children = match body {
                    None => Vec::new(),
                    Some(b) => match b.as_statements_node() {
                        Some(s) => s.body().iter().filter_map(|c| self.conv(&c, Some(i))).collect(),
                        None => self.conv(&b, Some(i)).into_iter().collect(),
                    },
                };
                self.nodes[i].children = children;
                i
            }
            Node::CallNode { .. } => self.call(&node.as_call_node().unwrap(), parent),
            Node::LambdaNode { .. } => {
                let l = node.as_lambda_node().unwrap();
                let i = self.push(Ty::Block, start, end, parent);
                let op = l.operator_loc();
                let lam = self.push(Ty::Other, op.start_offset(), op.end_offset(), Some(i));
                self.nodes[lam].name = "lambda".into();
                let mut children = vec![lam];
                let args = self.push(Ty::Other, start, start, Some(i));
                if let Some(p) = l.parameters() {
                    self.nodes[i].arg_names = param_names(&p);
                    self.nodes[i].has_args = !self.nodes[i].arg_names.is_empty();
                }
                children.push(args);
                let body = self.body(l.body(), Some(i));
                children.extend(body);
                self.nodes[i].children = children;
                self.nodes[i].second = body;
                i
            }
            Node::IfNode { .. } | Node::UnlessNode { .. } => {
                let i = self.push(Ty::If, start, end, parent);
                let (keyword, predicate, statements, subsequent) = match node {
                    Node::IfNode { .. } => {
                        let n = node.as_if_node().unwrap();
                        let kw = n.if_keyword_loc().map_or(String::new(), |k| String::from_utf8_lossy(k.as_slice()).into_owned());
                        (kw, n.predicate(), n.statements(), n.subsequent())
                    }
                    _ => {
                        let n = node.as_unless_node().unwrap();
                        ("unless".to_string(), n.predicate(), n.statements(), n.else_clause().map(|e| e.as_node()))
                    }
                };
                let cond = self.conv(&predicate, Some(i));
                let tb = self.body(statements.map(|s| s.as_node()), Some(i));
                let fb = match subsequent {
                    None => None,
                    Some(s) => match s.as_else_node() {
                        Some(e) => self.body(e.statements().map(|s| s.as_node()), Some(i)),
                        None => self.conv(&s, Some(i)),
                    },
                };
                let unless = keyword == "unless";
                let n = &mut self.nodes[i];
                n.children = if unless {
                    cond.into_iter().chain(fb).chain(tb).collect()
                } else {
                    cond.into_iter().chain(tb).chain(fb).collect()
                };
                n.name = keyword;
                n.first = cond;
                n.second = fb;
                i
            }
            Node::CaseNode { .. } => {
                let c = node.as_case_node().unwrap();
                let i = self.push(Ty::Case, start, end, parent);
                let cond = c.predicate().and_then(|p| self.conv(&p, Some(i)));
                let mut children: Vec<usize> = cond.into_iter().collect();
                for w in c.conditions().iter() {
                    children.extend(self.conv(&w, Some(i)));
                }
                children.extend(self.body(c.else_clause().and_then(|e| e.statements()).map(|s| s.as_node()), Some(i)));
                self.nodes[i].children = children;
                self.nodes[i].first = cond;
                i
            }
            Node::WhenNode { .. } => {
                let w = node.as_when_node().unwrap();
                let i = self.push(Ty::When, start, end, parent);
                let conds: Vec<usize> = w.conditions().iter().filter_map(|c| self.conv(&c, Some(i))).collect();
                let body = self.body(w.statements().map(|s| s.as_node()), Some(i));
                self.nodes[i].children = conds.iter().copied().chain(body).collect();
                self.nodes[i].list = conds;
                i
            }
            Node::AndNode { .. } | Node::OrNode { .. } => {
                let ty = if node.as_and_node().is_some() { Ty::And } else { Ty::Or };
                let i = self.generic(ty, node, parent);
                self.nodes[i].first = self.nodes[i].children.first().copied();
                i
            }
            Node::AssocNode { .. } => self.generic(Ty::Pair, node, parent),
            Node::LocalVariableWriteNode { .. }
            | Node::InstanceVariableWriteNode { .. }
            | Node::ClassVariableWriteNode { .. }
            | Node::GlobalVariableWriteNode { .. }
            | Node::ConstantWriteNode { .. }
            | Node::ConstantPathWriteNode { .. } => {
                let i = self.generic(Ty::Asgn, node, parent);
                // The value is the last child (a ConstantPathWriteNode's scope comes first).
                self.nodes[i].first = self.nodes[i].children.last().copied();
                i
            }
            Node::DefNode { .. }
            | Node::ClassNode { .. }
            | Node::ModuleNode { .. }
            | Node::SingletonClassNode { .. } => self.generic(Ty::Scope, node, parent),
            Node::LocalVariableReadNode { .. } | Node::ItLocalVariableReadNode { .. } => {
                let i = self.push(Ty::Lvar, start, end, parent);
                self.nodes[i].name = match node.as_local_variable_read_node() {
                    Some(l) => String::from_utf8_lossy(l.name().as_slice()).into_owned(),
                    None => "it".into(),
                };
                i
            }
            Node::BeginNode { .. } => self.begin(&node.as_begin_node().unwrap(), parent),
            Node::RescueModifierNode { .. } => {
                // `a rescue b`: (rescue a (resbody nil nil b) nil)
                let m = node.as_rescue_modifier_node().unwrap();
                let i = self.push(Ty::Rescue, start, end, parent);
                let expr = self.conv(&m.expression(), Some(i));
                let (s, e) = Self::span(&m.rescue_expression());
                let rb = self.push(Ty::Resbody, m.keyword_loc().start_offset(), e.max(s), Some(i));
                let rescued = self.conv(&m.rescue_expression(), Some(rb));
                self.nodes[rb].children = rescued.into_iter().collect();
                self.nodes[i].children = expr.into_iter().chain([rb]).collect();
                i
            }
            Node::RescueNode { .. } => {
                // Only reached through `begin`; each resbody is a sibling in the rescue node.
                self.generic(Ty::Resbody, node, parent)
            }
            _ => self.generic(Ty::Other, node, parent),
        })
    }

    fn begin(&mut self, b: &ruby_prism::BeginNode, parent: Option<usize>) -> usize {
        let (start, end) = Self::span(&b.as_node());
        let kw = b.begin_keyword_loc().is_some();
        let outer = if kw { Some(self.push(Ty::Kwbegin, start, end, parent)) } else { None };
        let holder = outer.or(parent);
        let has_rescue = b.rescue_clause().is_some();
        let has_ensure = b.ensure_clause().is_some();
        if !has_rescue && !has_ensure {
            let i = outer.expect("an implicit begin has rescue or ensure");
            let children = b.statements().map_or(Vec::new(), |s| {
                s.body().iter().filter_map(|c| self.conv(&c, Some(i))).collect()
            });
            self.nodes[i].children = children;
            return i;
        }
        let ensure = has_ensure.then(|| self.push(Ty::Ensure, start, end, holder));
        let rescue = has_rescue.then(|| self.push(Ty::Rescue, start, end, ensure.or(holder)));
        let body_parent = rescue.or(ensure);
        let body = self.body(b.statements().map(|s| s.as_node()), body_parent);
        if let Some(r) = rescue {
            let mut children: Vec<usize> = body.into_iter().collect();
            let mut clause = b.rescue_clause();
            while let Some(rc) = clause {
                let rb = rc.as_node();
                let (s, e) = Self::span(&rb);
                let i = self.push(Ty::Resbody, s, e, Some(r));
                let mut kids = Vec::new();
                let exceptions: Vec<Node> = rc.exceptions().iter().collect();
                if !exceptions.is_empty() {
                    let arr = self.push(Ty::Other, s, e, Some(i));
                    let elems = exceptions.iter().filter_map(|x| self.conv(x, Some(arr))).collect();
                    self.nodes[arr].children = elems;
                    kids.push(arr);
                }
                if let Some(reference) = rc.reference() {
                    kids.extend(self.conv(&reference, Some(i)));
                }
                kids.extend(self.body(rc.statements().map(|s| s.as_node()), Some(i)));
                self.nodes[i].children = kids;
                children.push(i);
                clause = rc.subsequent();
            }
            children.extend(self.body(b.else_clause().and_then(|e| e.statements()).map(|s| s.as_node()), Some(r)));
            self.nodes[r].children = children;
        }
        let top = match ensure {
            Some(en) => {
                let inner = rescue.or(body);
                let branch = self.body(b.ensure_clause().unwrap().statements().map(|s| s.as_node()), Some(en));
                self.nodes[en].children = inner.into_iter().chain(branch).collect();
                self.nodes[en].second = branch;
                en
            }
            None => rescue.unwrap(),
        };
        match outer {
            Some(k) => {
                self.nodes[k].children = vec![top];
                k
            }
            None => top,
        }
    }

    fn call(&mut self, call: &CallNode, parent: Option<usize>) -> usize {
        let loc = call.location();
        let block = call.block().and_then(|b| b.as_block_node());
        let block_idx = block.as_ref().map(|_| self.push(Ty::Block, loc.start_offset(), loc.end_offset(), parent));
        let (s, e) = send_range(call);
        let ty = if crate::ast::is_csend(call) { Ty::Csend } else { Ty::Send };
        let i = self.push(ty, s, e, block_idx.or(parent));
        self.nodes[i].name = method_name(call).to_string();
        let recv = call.receiver().and_then(|r| self.conv(&r, Some(i)));
        let mut args: Vec<usize> = call
            .arguments()
            .map_or(Vec::new(), |a| a.arguments().iter().filter_map(|x| self.conv(&x, Some(i))).collect());
        if let Some(bp) = call.block().filter(|b| b.as_block_argument_node().is_some()) {
            args.extend(self.conv(&bp, Some(i)));
        }
        self.nodes[i].children = recv.into_iter().chain(args.iter().copied()).collect();
        self.nodes[i].first = recv;
        self.nodes[i].list = args;
        let (Some(b), Some(bi)) = (block, block_idx) else { return i };
        let mut children = vec![i];
        match b.parameters() {
            Some(Node::NumberedParametersNode { .. }) => {
                let max = b.parameters().unwrap().as_numbered_parameters_node().unwrap().maximum();
                self.nodes[bi].arg_names = (1..=max).map(|k| format!("_{k}")).collect();
            }
            Some(Node::ItParametersNode { .. }) => self.nodes[bi].arg_names = vec!["it".into()],
            Some(p) => {
                let (ps, pe) = Self::span(&p);
                let args_node = self.push(Ty::Other, ps, pe, Some(bi));
                children.push(args_node);
                self.nodes[bi].arg_names = param_names(&p);
                self.nodes[bi].has_args = p
                    .as_block_parameters_node()
                    .is_some_and(|bp| bp.parameters().is_some() || bp.locals().iter().next().is_some());
            }
            None => {
                let args_node = self.push(Ty::Other, e, e, Some(bi));
                children.push(args_node);
            }
        }
        let body = self.body(b.body(), Some(bi));
        children.extend(body);
        self.nodes[bi].children = children;
        self.nodes[bi].second = body;
        self.nodes[bi].name = method_name(call).to_string();
        bi
    }
}

struct Checker<'a> {
    t: &'a Tree,
    src: &'a Source<'a>,
    additional: &'a [String],
    checked: HashSet<usize>,
    binding_name: Option<String>,
    binding_scope: Option<usize>,
}

impl Checker<'_> {
    fn n(&self, i: usize) -> &PNode {
        &self.t.nodes[i]
    }

    /// Structural `==` (token shape of the two ranges).
    fn same(&self, a: usize, b: usize) -> bool {
        let (a, b) = (self.n(a), self.n(b));
        super::node_equality::same(self.src, (a.start, a.end), (b.start, b.end))
    }

    fn binding_name(&self, i: usize) -> Option<String> {
        let n = self.n(i);
        match n.ty {
            Ty::Lvar => Some(n.name.clone()),
            Ty::Send if n.first.is_none() && n.name == "it" && n.list.is_empty() => Some("it".into()),
            _ => None,
        }
    }

    fn binding_scope(&self, occurrence: usize, name: &str) -> Option<usize> {
        let mut node = occurrence;
        while let Some(p) = self.n(node).parent {
            let pn = self.n(p);
            match pn.ty {
                Ty::Block => {
                    let binds = pn.arg_names.iter().any(|a| a == name) || (name == "it" && !pn.has_args);
                    if pn.second == Some(node) && binds {
                        return Some(p);
                    }
                }
                Ty::Scope => return Some(p),
                _ => {}
            }
            node = p;
        }
        None
    }

    fn same_binding(&self, occurrence: usize) -> bool {
        match &self.binding_name {
            None => true,
            Some(name) => self.binding_scope(occurrence, name) == self.binding_scope,
        }
    }

    fn non_nil_method(&self, name: &str) -> bool {
        !super::nil_methods::nil_object_method(name) && !self.additional.iter().any(|m| m == name)
    }

    fn else_branch(&self, i: usize) -> bool {
        let Some(p) = self.n(i).parent else { return false };
        self.n(p).ty == Ty::If && self.n(p).second.is_some_and(|e| self.same(e, i))
    }

    fn sequentially_reached(&self, i: usize) -> bool {
        let n = self.n(i);
        if n.ty == Ty::Resbody {
            return false;
        }
        if let Some(p) = n.parent
            && self.n(p).ty == Ty::Ensure && self.n(p).second == Some(i) {
                return false;
            }
        !self.else_branch(i) || (n.ty == Ty::If && n.name != "elsif")
    }

    fn walk(&mut self, node: Option<usize>, receiver: usize) -> bool {
        let Some(i) = node else { return false };
        if !self.checked.insert(i) {
            return false;
        }
        let ty = self.n(i).ty;
        match ty {
            Ty::Scope => return false,
            Ty::Send => {
                if let Some(r) = self.n(i).first
                    && self.same(r, receiver) && self.same_binding(r) {
                        return self.non_nil_method(&self.n(i).name.clone());
                    }
                for a in self.n(i).list.clone() {
                    if self.walk(Some(a), receiver) {
                        return true;
                    }
                }
                if self.walk(self.n(i).first, receiver) {
                    return true;
                }
            }
            Ty::Begin => {
                if self.walk(self.n(i).children.first().copied(), receiver) {
                    return true;
                }
            }
            Ty::If | Ty::Case | Ty::And | Ty::Or | Ty::Asgn => {
                if self.walk(self.n(i).first, receiver) {
                    return true;
                }
            }
            Ty::Pair => {
                let c = self.n(i).children.clone();
                if self.walk(c.first().copied(), receiver) || self.walk(c.get(1).copied(), receiver) {
                    return true;
                }
            }
            Ty::When => {
                for c in self.n(i).list.clone() {
                    if self.walk(Some(c), receiver) {
                        return true;
                    }
                }
            }
            _ => {}
        }
        if self.sequentially_reached(i)
            && let Some(p) = self.n(i).parent {
                let siblings = self.n(p).children.clone();
                let idx = siblings.iter().position(|&s| s == i).unwrap_or(0);
                for &s in siblings[..idx].iter().rev() {
                    if self.walk(Some(s), receiver) {
                        return true;
                    }
                }
            }
        self.walk(self.n(i).parent, receiver)
    }

    fn non_nil_condition(&self, condition: usize, node: usize) -> bool {
        if self.same(condition, node) && self.same_binding(condition) {
            return true;
        }
        if self.n(condition).ty != Ty::Csend {
            return false;
        }
        let Some(mut receiver) = self.n(condition).first else { return false };
        while matches!(self.n(receiver).ty, Ty::Send | Ty::Csend) {
            match self.n(receiver).first {
                Some(r) => receiver = r,
                None => break,
            }
        }
        self.same(receiver, node) && self.same_binding(receiver)
    }

    fn sole_condition_of_parent_if(&self, node: usize) -> bool {
        let mut child = node;
        let mut parent = self.n(node).parent;
        while let Some(mut p) = parent {
            if self.n(p).ty == Ty::If {
                if self.n(p).name != "unless"
                    && let Some(c) = self.n(p).first
                        && c != child && self.non_nil_condition(c, node) {
                            return true;
                        }
                if self.n(p).name == "elsif" {
                    while self.n(p).name == "elsif" && self.n(p).ty == Ty::If {
                        match self.n(p).parent {
                            Some(pp) => p = pp,
                            None => break,
                        }
                    }
                }
            } else if self.else_branch(p) {
                p = self.n(p).parent.unwrap();
            }
            child = p;
            parent = self.n(p).parent;
        }
        false
    }
}

/// `NilReceiverChecker.new(call.receiver, additional).cant_be_nil?` for a `&.` call.
/// ponytail: rebuilds the parser-shaped tree per call (InferNonNilReceiver is opt-in); cache it
/// per file if that ever shows up in profiles.
pub fn cant_be_nil(src: &Source, ancestors: &[Node], call: &CallNode, additional: &[String]) -> bool {
    let Some(root) = ancestors.first() else { return false };
    let mut t = Tree { nodes: Vec::new() };
    t.conv(root, None);
    let (s, e) = send_range(call);
    let Some(csend) = t.nodes.iter().position(|n| n.ty == Ty::Csend && (n.start, n.end) == (s, e)) else {
        return false;
    };
    let Some(receiver) = t.nodes[csend].first else { return false };
    let mut c =
        Checker { t: &t, src, additional, checked: HashSet::new(), binding_name: None, binding_scope: None };
    c.binding_name = c.binding_name(receiver);
    if let Some(name) = c.binding_name.clone() {
        c.binding_scope = c.binding_scope(receiver, &name);
    }
    c.sole_condition_of_parent_if(receiver) || c.walk(t.nodes[receiver].parent, receiver)
}

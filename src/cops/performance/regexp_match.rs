use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Performance/RegexpMatch";
pub static COP: RegexpMatch = RegexpMatch;
pub struct RegexpMatch;

const MATCH_GVARS: &[&[u8]] = &[b"$~", b"$MATCH", b"$PREMATCH", b"$POSTMATCH", b"$LAST_PAREN_MATCH", b"$LAST_MATCH_INFO"];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// match_method? / match_with_int_arg_method?
    Method,
    /// match_operator? / match_threequals?
    Operator,
    /// match_with_lvasgn? (`/re/ =~ x` without named captures)
    Lvasgn,
}

fn regexp(node: &Node) -> bool {
    matches!(kind::of(node), kind::REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE)
}

/// `{regexp str sym}` (TYPES_IMPLEMENTING_MATCH adds `const`).
fn literal(node: &Node, target: f64) -> bool {
    regexp(node)
        || (matches!(kind::of(node), kind::STRING_NODE | kind::SOURCE_FILE_NODE) && !is_parser_dstr(node))
        || (node.as_symbol_node().is_some() && !is_parser_dsym(node, target))
}

/// MATCH_NODE_PATTERN on a node, with parser semantics.
fn match_kind(node: &Node, target: f64) -> Option<Kind> {
    let call = node.as_call_node()?;
    if is_csend(&call) || call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return None;
    }
    let recv = call.receiver();
    let args = send_args(&call);
    match method_name(&call) {
        "match" => {
            let one = args.len() == 1;
            if (one && literal(&args[0], target))
                || (one && recv.as_ref().is_some_and(|r| literal(r, target)))
                || (args.len() == 2 && args[1].as_integer_node().is_some())
            {
                return Some(Kind::Method);
            }
            None
        }
        // `/re/ =~ x` is a match-with-lvasgn: a match unless the regexp has named captures.
        "=~" if recv.as_ref().is_some_and(regexp) => {
            if args.len() != 1 {
                return None;
            }
            let recv = recv.unwrap();
            let (content, extended) = match recv.as_regular_expression_node() {
                Some(r) => (text(&r.content_loc()).to_string(), r.is_extended()),
                None => {
                    let r = recv.as_interpolated_regular_expression_node().unwrap();
                    let parts: String = r
                        .parts()
                        .iter()
                        .filter_map(|p| p.as_string_node().map(|s| text(&s.content_loc()).to_string()))
                        .collect();
                    (parts, r.is_extended())
                }
            };
            let (named, _) = helpers::regexp_captures::count_captures(content.as_bytes(), extended);
            (named == 0).then_some(Kind::Lvasgn)
        }
        "=~" | "!~" => (recv.is_some() && args.len() == 1).then_some(Kind::Operator),
        // (send (regexp (str _) {(regopt) (regopt _)}) :=== !nil?)
        "===" => {
            let r = recv?.as_regular_expression_node()?;
            let content = text(&r.content_loc());
            let body = content.strip_suffix('\n').unwrap_or(content);
            let one_str = !content.is_empty() && !body.contains('\n');
            (one_str && r.closing_loc().as_slice().len() <= 2 && args.len() == 1).then_some(Kind::Operator)
        }
        _ => None,
    }
}

impl Cop for RegexpMatch {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.4, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::CASE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match node {
            Node::IfNode { .. } if !is_pattern_guard(node, cx.ancestors) => {
                check(cx, node, &node.as_if_node().unwrap().predicate());
            }
            Node::UnlessNode { .. } if !is_pattern_guard(node, cx.ancestors) => {
                check(cx, node, &node.as_unless_node().unwrap().predicate());
            }
            Node::CaseNode { .. } => {
                let case = node.as_case_node().unwrap();
                if case.predicate().is_some() {
                    return;
                }
                for when in case.conditions().iter() {
                    let Some(when) = when.as_when_node() else { continue };
                    for cond in when.conditions().iter() {
                        check(cx, node, &cond);
                    }
                }
            }
            _ => {}
        }
    }
}

fn check(cx: &mut Cx, owner: &Node, cond: &Node) {
    let target = cx.config.target_ruby;
    let Some(kind) = match_kind(cond, target) else { return };
    let call = cond.as_call_node().unwrap();
    // scope_root: the nearest def, class or module; its body (or the whole file) is searched.
    let scope = cx.ancestors.iter().rev().find(|a| {
        matches!(kind::of(a), kind::DEF_NODE | kind::CLASS_NODE | kind::MODULE_NODE)
    });
    let scope_id = scope.map(|s| s.location().start_offset());
    let body: Option<Node> = match scope {
        Some(s) => match s {
            Node::DefNode { .. } => s.as_def_node().unwrap().body(),
            Node::ClassNode { .. } => s.as_class_node().unwrap().body(),
            _ => s.as_module_node().unwrap().body(),
        },
        None => cx.ancestors.first().map(|p| p.as_program_node().unwrap().statements().as_node()),
    };
    let Some(body) = body else { return };
    let mut scan = Scan { target, scopes: vec![scope_id], matches: Vec::new(), refs: Vec::new(), modifier_branch: Vec::new(), entered: Vec::new() };
    scan.visit(&body);
    let start = cond.location().start_offset();
    let adjusted = |s: usize| scan.modifier_branch.iter().find(|(p, _)| *p == s).map_or(s, |(_, b)| *b);
    // range_to_search_for_last_matches: from the match (or a modifier if's branch) to the next
    // match in the same scope.
    let pos = if is_modifier(owner, cond) { modifier_branch_start(owner).unwrap_or(start) } else { start };
    let next = scan
        .matches
        .iter()
        .find(|&&(s, sc)| adjusted(s) > pos && sc == scope_id)
        .map_or(usize::MAX, |&(s, _)| s);
    if scan.refs.iter().any(|&(r, sc)| pos <= r && r <= next && sc == scope_id) {
        return;
    }
    let negation = if kind == Kind::Operator && method_name(&call) == "!~" { "!" } else { "" };
    let selector = call.message_loc().map_or("", |l| text(&l));
    let msg = format!("Use `{negation}match?` instead of `{selector}` when `MatchData` is not used.");
    let (s, e) = send_range(&call);
    match kind {
        Kind::Method => {
            let sel = call.message_loc().unwrap();
            cx.add_fix(s, e, msg, |c| c.replace(sel.start_offset(), sel.end_offset(), "match?"));
        }
        Kind::Operator | Kind::Lvasgn => {
            let recv = call.receiver().unwrap();
            let args = send_args(&call);
            let arg = &args[0];
            let (rs, re) = (recv.location().start_offset(), recv.location().end_offset());
            let (as_, ae) = (arg.location().start_offset(), arg.location().end_offset());
            let op_src = cx.src.slice(re, as_);
            let implementing = |n: &Node| literal(n, target) || matches!(kind::of(n), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE);
            let (op_text, swap) = if implementing(&recv) {
                (".match?(", false)
            } else if implementing(arg) {
                (".match?(", true)
            } else {
                ("&.match?(", false)
            };
            let close = !op_src.ends_with('(');
            let bang = method_name(&call) == "!~";
            let (recv_src, arg_src) = (node_text(&recv).to_string(), node_text(arg).to_string());
            cx.add_fix(s, e, msg, |c| {
                c.replace(re, as_, op_text);
                if swap {
                    c.replace(rs, re, &arg_src);
                    c.replace(as_, ae, &recv_src);
                }
                if close {
                    c.insert_after(as_, ae, ")");
                }
                if bang {
                    c.insert_before(rs, re, "!");
                }
            });
        }
    }
}

/// modifier_form?(match_node): its parent is a modifier if/unless.
fn is_modifier(owner: &Node, cond: &Node) -> bool {
    let start = cond.location().start_offset();
    match owner {
        Node::IfNode { .. } => {
            let n = owner.as_if_node().unwrap();
            n.predicate().location().start_offset() == start && n.if_keyword_loc().is_some() && n.end_keyword_loc().is_none()
        }
        Node::UnlessNode { .. } => {
            let n = owner.as_unless_node().unwrap();
            n.predicate().location().start_offset() == start && n.end_keyword_loc().is_none()
        }
        _ => false,
    }
}

/// `parent.if_branch.source_range.begin_pos` of a modifier if/unless.
fn modifier_branch_start(owner: &Node) -> Option<usize> {
    match owner {
        Node::IfNode { .. } => owner.as_if_node().unwrap().statements().map(|s| s.location().start_offset()),
        Node::UnlessNode { .. } => owner.as_unless_node().unwrap().statements().map(|s| s.location().start_offset()),
        _ => None,
    }
}

/// search_match_nodes and last_matches over a scope body, each with its scope root.
struct Scan {
    target: f64,
    scopes: Vec<Option<usize>>,
    /// (begin_pos, scope) of match nodes, in pre-order.
    matches: Vec<(usize, Option<usize>)>,
    /// (begin_pos, scope) of last-match references.
    refs: Vec<(usize, Option<usize>)>,
    /// (match begin, branch begin) for matches that are a modifier if's condition.
    modifier_branch: Vec<(usize, usize)>,
    /// Whether each entered branch node opened a scope.
    entered: Vec<bool>,
}

impl Scan {
    fn scope(&self) -> Option<usize> {
        *self.scopes.last().unwrap()
    }
}

impl<'pr> Visit<'pr> for Scan {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        let start = node.location().start_offset();
        if match_kind(&node, self.target).is_some() {
            self.matches.push((start, self.scope()));
        }
        // (send (const {nil? cbase} :Regexp) :last_match _?)
        if let Some(c) = node.as_call_node()
            && !is_csend(&c)
            && method_name(&c) == "last_match"
            && send_args(&c).len() <= 1
            && c.block().is_none_or(|b| b.as_block_argument_node().is_some())
            && c.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Regexp"))
        {
            self.refs.push((start, self.scope()));
        }
        match &node {
            Node::IfNode { .. } => {
                let n = node.as_if_node().unwrap();
                if n.if_keyword_loc().is_some() && n.end_keyword_loc().is_none()
                    && let Some(s) = n.statements()
                {
                    self.modifier_branch.push((n.predicate().location().start_offset(), s.location().start_offset()));
                }
            }
            Node::UnlessNode { .. } => {
                let n = node.as_unless_node().unwrap();
                if n.end_keyword_loc().is_none()
                    && let Some(s) = n.statements()
                {
                    self.modifier_branch.push((n.predicate().location().start_offset(), s.location().start_offset()));
                }
            }
            _ => {}
        }
        let scope = matches!(node, Node::DefNode { .. } | Node::ClassNode { .. } | Node::ModuleNode { .. });
        if scope {
            self.scopes.push(Some(start));
        }
        self.entered.push(scope);
    }

    fn visit_branch_node_leave(&mut self) {
        if self.entered.pop() == Some(true) {
            self.scopes.pop();
        }
    }

    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        let start = node.location().start_offset();
        let is_ref = match &node {
            Node::BackReferenceReadNode { .. } | Node::NumberedReferenceReadNode { .. } => true,
            Node::GlobalVariableReadNode { .. } => {
                MATCH_GVARS.contains(&node.as_global_variable_read_node().unwrap().name().as_slice())
            }
            _ => false,
        };
        if is_ref {
            self.refs.push((start, self.scope()));
        }
    }
}

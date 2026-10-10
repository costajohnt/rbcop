use crate::prelude::*;
use crate::source::Source;
use helpers::if_node::{is_elsif, parts, Body, Else};
use ruby_prism::Visit;
use std::sync::Arc;

pub const NAME: &str = "Style/ConditionalAssignment";
pub static COP: ConditionalAssignment = ConditionalAssignment;
pub struct ConditionalAssignment;

const MSG: &str = "Use the return of the conditional for variable assignment and comparison.";
const ASSIGN_TO_CONDITION_MSG: &str = "Assign variables inside of conditionals.";

#[derive(Default)]
struct State {
    /// ignore_node ranges (assign_inside_condition).
    ignored: Vec<(usize, usize)>,
    source: Option<Arc<str>>,
}

/// One corrector call; positions are byte offsets.
enum Edit {
    Replace(usize, usize, String),
    Remove(usize, usize),
    InsertBefore(usize, usize, String),
}

fn apply(c: &mut Corrector, edits: &[Edit]) {
    for e in edits {
        match e {
            Edit::Replace(s, e, t) => c.replace(*s, *e, t),
            Edit::Remove(s, e) => c.remove(*s, *e),
            Edit::InsertBefore(s, e, t) => c.insert_before(*s, *e, t),
        }
    }
}

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// A plain `send` (not csend, no block literal).
fn send<'pr>(n: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    n.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// Parser type of an `assignment_type?` node, None otherwise.
fn asg_type(n: &Node) -> Option<&'static str> {
    if let Some(c) = send(n) {
        let m = method_name(&c);
        let ok = matches!(m, "[]=" | "<<" | "=~" | "!~" | "<=>" | "<" | ">") || m.ends_with('=');
        return ok.then_some("send");
    }
    Some(match kind::of(n) {
        kind::LOCAL_VARIABLE_WRITE_NODE => "lvasgn",
        kind::INSTANCE_VARIABLE_WRITE_NODE => "ivasgn",
        kind::CLASS_VARIABLE_WRITE_NODE => "cvasgn",
        kind::GLOBAL_VARIABLE_WRITE_NODE => "gvasgn",
        kind::CONSTANT_WRITE_NODE | kind::CONSTANT_PATH_WRITE_NODE => "casgn",
        kind::MULTI_WRITE_NODE => "masgn",
        k if ASSIGNMENT_KINDS.contains(&k) => {
            let (op, _) = assignment_parts(n)?;
            match op.as_slice() {
                b"||=" => "or_asgn",
                b"&&=" => "and_asgn",
                _ => "op_asgn",
            }
        }
        _ => return None,
    })
}

/// rubocop-ast `const_name` of a namespace node.
fn const_name(n: &Node) -> Option<String> {
    if let Some(c) = n.as_constant_read_node() {
        return Some(String::from_utf8_lossy(c.name().as_slice()).into_owned());
    }
    let p = n.as_constant_path_node()?;
    let name = String::from_utf8_lossy(p.name()?.as_slice()).into_owned();
    Some(match p.parent() {
        None => name,
        Some(ns) => format!("{}::{name}", const_name(&ns).unwrap_or_default()),
    })
}

/// ConditionalAssignmentHelper#lhs.
fn lhs(src: &Source, n: &Node) -> String {
    let name = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    if let Some(c) = send(n) {
        let recv = c.receiver().map_or("", |r| node_text(&r));
        let m = method_name(&c);
        if m == "[]=" {
            let args = send_args(&c);
            let idx: Vec<&str> = args[..args.len().saturating_sub(1)].iter().map(node_text).collect();
            return format!("{recv}[{}] = ", idx.join(", "));
        }
        if c.equal_loc().is_some() {
            return format!("{recv}.{} = ", &m[..m.len() - 1]);
        }
        return format!("{recv} {m} ");
    }
    match kind::of(n) {
        kind::LOCAL_VARIABLE_WRITE_NODE => format!("{} = ", name(n.as_local_variable_write_node().unwrap().name().as_slice())),
        kind::INSTANCE_VARIABLE_WRITE_NODE => {
            format!("{} = ", name(n.as_instance_variable_write_node().unwrap().name().as_slice()))
        }
        kind::CLASS_VARIABLE_WRITE_NODE => format!("{} = ", name(n.as_class_variable_write_node().unwrap().name().as_slice())),
        kind::GLOBAL_VARIABLE_WRITE_NODE => {
            format!("{} = ", name(n.as_global_variable_write_node().unwrap().name().as_slice()))
        }
        kind::CONSTANT_WRITE_NODE => format!("{} = ", name(n.as_constant_write_node().unwrap().name().as_slice())),
        kind::CONSTANT_PATH_WRITE_NODE => {
            let t = n.as_constant_path_write_node().unwrap().target();
            let short = t.name().map_or(String::new(), |x| name(x.as_slice()));
            match t.parent() {
                None => format!("::{short} = "),
                Some(ns) => format!("{}::{short} = ", const_name(&ns).unwrap_or_default()),
            }
        }
        kind::MULTI_WRITE_NODE => node_text(n).to_string(),
        k if ASSIGNMENT_KINDS.contains(&k) => {
            let (op, _) = assignment_parts(n).unwrap();
            let target = src.slice(n.location().start_offset(), op.start_offset()).trim_end();
            format!("{target} {} ", text(&op))
        }
        _ => node_text(n).to_string(),
    }
}

/// `*_, assignment = *node`: the last child node (None where Ruby would get a non-node).
fn last_child<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    if let Some(c) = n.as_call_node() {
        if let Some(b) = c.block().filter(|b| b.as_block_argument_node().is_some()) {
            return Some(b);
        }
        return c.arguments().and_then(|a| a.arguments().iter().last());
    }
    if let Some(p) = n.as_parentheses_node() {
        let body = p.body()?;
        return match body.as_statements_node() {
            Some(s) => s.body().iter().last(),
            None => Some(body),
        };
    }
    if let Some(m) = n.as_multi_write_node() {
        return Some(m.value());
    }
    assignment_parts(n).map(|(_, v)| v)
}

/// replace_branch_assignment (`bracket`) / correct_branches.
fn replace_branch(edits: &mut Vec<Edit>, n: &Node, bracket: bool) -> Option<()> {
    let value = last_child(n)?;
    let mut text = node_text(&value).to_string();
    if bracket && value.as_array_node().is_some_and(|a| a.opening_loc().is_none()) {
        text = format!("[{text}]");
    }
    let (s, e) = range(n);
    edits.push(Edit::Replace(s, e, text));
    Some(())
}

/// The `when`/`in` bodies and the else clause of a case/case_match.
/// (`when`/`in` bodies with their keyword starts, the else clause, the `end` start).
type CaseParts<'pr> = (Vec<(Option<Body<'pr>>, usize)>, Option<ruby_prism::ElseNode<'pr>>, usize);

fn case_parts<'pr>(node: &Node<'pr>) -> CaseParts<'pr> {
    if let Some(c) = node.as_case_node() {
        let whens = c
            .conditions()
            .iter()
            .map(|w| {
                let w = w.as_when_node().unwrap();
                (Body::new(w.statements()), w.keyword_loc().start_offset())
            })
            .collect();
        return (whens, c.else_clause(), c.end_keyword_loc().start_offset());
    }
    let c = node.as_case_match_node().unwrap();
    let ins = c
        .conditions()
        .iter()
        .map(|i| {
            let i = i.as_in_node().unwrap();
            (Body::new(i.statements()), i.in_loc().start_offset())
        })
        .collect();
    (ins, c.else_clause(), c.end_keyword_loc().start_offset())
}

/// `else_branch` (the else body) of a case/case_match.
fn else_body<'pr>(e: &Option<ruby_prism::ElseNode<'pr>>) -> Option<Body<'pr>> {
    e.as_ref().and_then(|e| Body::new(e.statements()))
}

/// A branch of an if chain with the start of its parent's `loc.else` (else/elsif keyword).
struct IfBranch<'pr> {
    body: Option<Body<'pr>>,
    parent_else: Option<usize>,
}

/// Start of an if/unless node's `loc.else`.
fn loc_else(node: &Node) -> Option<usize> {
    if let Some(n) = node.as_if_node() {
        let s = n.subsequent()?;
        return Some(s.as_else_node().map_or(s.location().start_offset(), |e| e.else_keyword_loc().start_offset()));
    }
    node.as_unless_node().unwrap().else_clause().map(|e| e.else_keyword_loc().start_offset())
}

/// IfNode#branches (elsif chains flattened), each with its parent's `loc.else`.
fn if_branches<'pr>(node: &Node<'pr>, out: &mut Vec<IfBranch<'pr>>) {
    let (_, if_branch, els) = parts(node);
    let parent_else = loc_else(node);
    out.push(IfBranch { body: if_branch, parent_else });
    match els {
        None => {}
        Some(Else::Elsif(n)) => if_branches(&n, out),
        Some(Else::Body(b)) => out.push(IfBranch { body: b, parent_else }),
    }
}

fn end_keyword(node: &Node) -> Option<usize> {
    match kind::of(node) {
        kind::IF_NODE => node.as_if_node().unwrap().end_keyword_loc().map(|l| l.start_offset()),
        kind::UNLESS_NODE => node.as_unless_node().unwrap().end_keyword_loc().map(|l| l.start_offset()),
        kind::CASE_NODE => Some(node.as_case_node().unwrap().end_keyword_loc().start_offset()),
        kind::CASE_MATCH_NODE => Some(node.as_case_match_node().unwrap().end_keyword_loc().start_offset()),
        _ => None,
    }
}

fn is_ternary(n: &Node) -> bool {
    n.as_if_node().is_some_and(|i| i.if_keyword_loc().is_none())
}

/// `Range#begin_pos - n` characters, as a byte offset (None before the start of the file).
fn back_chars(text: &str, pos: usize, n: usize) -> Option<usize> {
    if n == 0 {
        return Some(pos);
    }
    text[..pos].char_indices().rev().nth(n - 1).map(|(i, _)| i)
}

/// `corrector.remove_preceding(range, range.column - column)`; None where Ruby raises.
fn remove_preceding(src: &Source, edits: &mut Vec<Edit>, pos: usize, column: usize) -> Option<()> {
    let size = src.column(pos).checked_sub(column)?;
    let start = back_chars(src.text, pos, size)?;
    edits.push(Edit::Remove(start, pos));
    Some(())
}

/// Parser-node starts in a branch for remove_whitespace_in_branches (`each_node`, children of a
/// `dstr` skipped). Prism nodes starting where parser has no node (`else`, `ensure`, a block's
/// `do`/`{`, an implicit begin) are left out.
struct Starts {
    out: Vec<usize>,
    in_dstr: bool,
}

impl<'pr> Visit<'pr> for Starts {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.record(&node);
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.record(&node);
    }
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
        for part in node.parts().iter() {
            self.in_dstr = true;
            self.visit(&part);
        }
        self.in_dstr = false;
    }
    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        self.in_dstr = false;
        ruby_prism::visit_statements_node(self, node);
    }
    // A BeginNode's `rescue_clause` is a typed field: the generic enter hook does not see it. Its
    // start (`rescue`) is the parser `resbody`'s.
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode<'pr>) {
        self.record(&node.as_node());
        ruby_prism::visit_rescue_node(self, node);
    }
}

impl Starts {
    fn record(&mut self, node: &Node) {
        let skip = self.in_dstr
            || match kind::of(node) {
                kind::ELSE_NODE | kind::ENSURE_NODE | kind::BLOCK_NODE => true,
                kind::BEGIN_NODE => node.as_begin_node().unwrap().begin_keyword_loc().is_none(),
                _ => false,
            };
        self.in_dstr = false;
        if !skip {
            self.out.push(node.location().start_offset());
        }
    }

    fn collect(body: &Body) -> Vec<usize> {
        let mut v = Starts { out: vec![], in_dstr: false };
        let nodes: Vec<Node> = body.stmts.body().iter().collect();
        if nodes.len() > 1 {
            v.out.push(body.range().0);
        }
        for n in &nodes {
            v.visit(n);
        }
        v.out
    }
}

/// remove_whitespace_in_branches; None where Ruby raises.
fn remove_whitespace_in_branches(
    src: &Source,
    edits: &mut Vec<Edit>,
    branch: &Body,
    column: usize,
    cond_else: Option<(usize, Option<usize>)>,
    cond_start: usize,
    cond_end: Option<(usize, Option<usize>)>,
) -> Option<()> {
    for start in Starts::collect(branch) {
        let col = src.column(start);
        let Some(size) = col.checked_sub(column + 2) else { continue };
        let Some(b) = back_chars(src.text, start, size) else { continue };
        if helpers::ruby_string::strip(src.slice(b, start)).is_empty() && b < start {
            edits.push(Edit::Remove(b, start));
        }
    }
    // condition.loc.else && !same_line?(condition.else_branch, condition)
    if let Some((else_kw, else_branch_start)) = cond_else
        && else_branch_start.is_none_or(|s| src.line_of(s) != src.line_of(cond_start))
    {
        remove_preceding(src, edits, else_kw, column)?;
    }
    // condition.loc.end && !same_line?(branches.last.parent.else_branch, condition.loc.end)
    if let Some((end_kw, last_else_start)) = cond_end
        && last_else_start.is_none_or(|s| src.line_of(s) != src.line_of(end_kw))
    {
        remove_preceding(src, edits, end_kw, column)?;
    }
    Some(())
}

impl Cop for ConditionalAssignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::IF_NODE,
            kind::UNLESS_NODE,
            kind::CASE_NODE,
            kind::CASE_MATCH_NODE,
            kind::CALL_NODE,
            kind::LOCAL_VARIABLE_WRITE_NODE,
            kind::INSTANCE_VARIABLE_WRITE_NODE,
            kind::CLASS_VARIABLE_WRITE_NODE,
            kind::GLOBAL_VARIABLE_WRITE_NODE,
            kind::CONSTANT_WRITE_NODE,
            kind::CONSTANT_PATH_WRITE_NODE,
            kind::MULTI_WRITE_NODE,
            kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
            kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::CONSTANT_OPERATOR_WRITE_NODE,
            kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
            kind::CALL_OPERATOR_WRITE_NODE,
            kind::INDEX_OPERATOR_WRITE_NODE,
            kind::LOCAL_VARIABLE_OR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
            kind::CLASS_VARIABLE_OR_WRITE_NODE,
            kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
            kind::CONSTANT_OR_WRITE_NODE,
            kind::CONSTANT_PATH_OR_WRITE_NODE,
            kind::CALL_OR_WRITE_NODE,
            kind::INDEX_OR_WRITE_NODE,
            kind::LOCAL_VARIABLE_AND_WRITE_NODE,
            kind::INSTANCE_VARIABLE_AND_WRITE_NODE,
            kind::CLASS_VARIABLE_AND_WRITE_NODE,
            kind::GLOBAL_VARIABLE_AND_WRITE_NODE,
            kind::CONSTANT_AND_WRITE_NODE,
            kind::CONSTANT_PATH_AND_WRITE_NODE,
            kind::CALL_AND_WRITE_NODE,
            kind::INDEX_AND_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let inside = cx.cfg.str("EnforcedStyle") == Some("assign_inside_condition");
        let conditional =
            matches!(kind::of(node), kind::IF_NODE | kind::UNLESS_NODE | kind::CASE_NODE | kind::CASE_MATCH_NODE);
        if conditional {
            if !inside && !is_pattern_guard(node, cx.ancestors) {
                check_conditional(cx, node);
            }
        } else if inside {
            check_assignment(cx, node);
        }
    }
}

fn single_line_only(cx: &Cx) -> bool {
    cx.cfg.bool("SingleLineConditionsOnly") == Some(true)
}

fn allowed_ternary(cx: &Cx, n: &Node) -> bool {
    is_ternary(n) && cx.cfg.bool("IncludeTernaryExpressions") != Some(true)
}

fn indent(cx: &Cx, lhs: &str) -> String {
    let conf = cx.config.for_cop("Layout/EndAlignment");
    if conf.str("EnforcedStyleAlignWith") == Some("keyword") { " ".repeat(lhs.chars().count()) } else { String::new() }
}

/// safe_to_correct?: the correction applied alone still parses.
fn correction_parses(cx: &mut Cx, edits: &[Edit]) -> bool {
    let src = cx.src;
    let state = cx.state::<State>();
    let source = state.source.get_or_insert_with(|| Arc::from(src.text)).clone();
    helpers::reparsed_equivalence::correction_parses(source, src.version, |c| apply(c, edits))
}

fn report(cx: &mut Cx, node_range: (usize, usize), msg: &str, edits: Option<Vec<Edit>>) {
    let Some(edits) = edits else { return };
    if !correction_parses(cx, &edits) {
        return;
    }
    cx.add_fix(node_range.0, node_range.1, msg, |c| apply(c, &edits));
}

/// on_if / on_case / on_case_match (assign_to_condition).
fn check_conditional(cx: &mut Cx, node: &Node) {
    let src = cx.src;
    let mut branches: Vec<Option<Body>> = vec![];
    if kind::of(node) == kind::CASE_NODE || kind::of(node) == kind::CASE_MATCH_NODE {
        let (whens, els, _) = case_parts(node);
        let Some(e) = else_body(&els) else { return };
        branches.extend(whens.into_iter().map(|(b, _)| b));
        branches.push(Some(e));
    } else {
        if is_elsif(node) {
            return;
        }
        let mut out = vec![];
        if_branches(node, &mut out);
        // expand_elses: the chain must end in an `else`.
        let mut els = parts(node).2;
        while let Some(Else::Elsif(n)) = els {
            els = parts(&n).2;
        }
        if els.is_none() {
            return;
        }
        branches = out.into_iter().map(|b| b.body).collect();
        if allowed_ternary(cx, node) {
            return;
        }
    }
    // allowed_statements?: every branch present, tails with matching lhs and assignment type.
    if branches.iter().any(Option::is_none) {
        return;
    }
    let branches: Vec<Body> = branches.into_iter().flatten().collect();
    let tails: Vec<Option<Node>> = branches.iter().map(Body::tail).collect();
    // A nil tail makes lhs/the corrector raise (no offense).
    if tails.iter().any(Option::is_none) {
        return;
    }
    let tails: Vec<Node> = tails.into_iter().flatten().collect();
    let first_lhs = lhs(src, &tails[0]);
    if tails.iter().any(|t| lhs(src, t) != first_lhs || t.as_multi_write_node().is_some()) {
        return;
    }
    let Some(ty) = asg_type(&tails[0]) else { return };
    if tails.iter().any(|t| asg_type(t) != Some(ty)) {
        return;
    }
    if single_line_only(cx) && branches.iter().any(Body::is_begin) {
        return;
    }
    if exceeds_line_limit(cx, node, &first_lhs) {
        return;
    }
    let (start, end) = range(node);
    let edits = move_outside(cx, node, &branches, &tails);
    report(cx, (start, end), MSG, edits);
}

fn exceeds_line_limit(cx: &Cx, node: &Node, assignment: &str) -> bool {
    let ll = cx.config.for_cop("Layout/LineLength");
    if !ll.truthy("Enabled") {
        return false;
    }
    let max = ll.int("Max").unwrap_or(120) as usize;
    let mut pat = String::from(r"[\t\n\v\f\r ]*");
    for ch in assignment.chars() {
        if ch == ' ' {
            pat.push_str(r"[\t\n\v\f\r ]*");
        } else {
            pat.push_str(&regex::escape(&ch.to_string()));
        }
    }
    let Ok(re) = Regex::new(&pat) else { return false };
    let longest = node_text(node)
        .split_inclusive('\n')
        .map(|l| {
            let l = l.strip_suffix('\n').unwrap_or(l);
            let l = l.strip_suffix('\r').unwrap_or(l);
            re.replacen(l, 1, "").chars().count()
        })
        .max()
        .unwrap_or(0);
    assignment.chars().count() + longest > max
}

/// move_assignment_outside_condition.
fn move_outside(cx: &Cx, node: &Node, branches: &[Body], tails: &[Node]) -> Option<Vec<Edit>> {
    let src = cx.src;
    let mut edits = vec![];
    let (start, end) = range(node);
    if is_ternary(node) {
        // TernaryCorrector: lhs(node.if_branch) + ternary.
        let if_branch = branches[0].single()?;
        let else_branch = branches[1].single()?;
        let (cond, _, _) = parts(node);
        let if_rhs = last_child(&if_branch)?;
        let else_rhs = last_child(&else_branch)?;
        let expr = format!("{} ? {} : {}", node_text(&cond), node_text(&if_rhs), node_text(&else_rhs));
        let element = send(&if_branch).is_some_and(|c| method_name(&c) != "[]=");
        let expr = if element { format!("({expr})") } else { expr };
        edits.push(Edit::Replace(start, end, format!("{}{expr}", lhs(src, &if_branch))));
        return Some(edits);
    }
    let end_kw = end_keyword(node)?;
    let is_case = matches!(kind::of(node), kind::CASE_NODE | kind::CASE_MATCH_NODE);
    // Case: lhs of the else branch; if: lhs of the if branch.
    let (lead, last) = (if is_case { tails.len() - 1 } else { 0 }, tails.len() - 1);
    let l = lhs(src, &tails[lead]);
    edits.push(Edit::InsertBefore(start, end, l.clone()));
    for (i, t) in tails.iter().enumerate() {
        // replace_branch_assignment for the if and else branches, correct_branches otherwise.
        let bracket = i == last || (!is_case && i == 0);
        replace_branch(&mut edits, t, bracket)?;
    }
    let kw_end = end_kw + 3;
    edits.push(Edit::InsertBefore(end_kw, kw_end, indent(cx, &l)));
    Some(edits)
}

/// on_<assignment> / on_send (assign_inside_condition).
fn check_assignment(cx: &mut Cx, node: &Node) {
    let is_send = node.as_call_node().is_some();
    if is_send {
        if asg_type(node) != Some("send") {
            return;
        }
    } else {
        let r = range(node);
        if cx.state::<State>().ignored.iter().any(|&(a, b)| a <= r.0 && r.1 <= b) {
            return;
        }
    }
    let r = range(node);
    cx.state::<State>().ignored.push(r);
    let Some(rhs) = last_child(node) else { return };
    // `(cond)` with one child unwraps to the conditional.
    let assignment = one_child(&rhs).unwrap_or_else(|| last_child(node).unwrap());
    let k = kind::of(&assignment);
    if !matches!(k, kind::IF_NODE | kind::UNLESS_NODE | kind::CASE_NODE | kind::CASE_MATCH_NODE) {
        return;
    }
    if allowed_ternary(cx, &assignment) {
        return;
    }
    let src = cx.src;
    // `_condition, *branches, else_branch = *assignment` and `assignment.else_branch`.
    let any_begin = if k == kind::CASE_NODE || k == kind::CASE_MATCH_NODE {
        let (_, els, _) = case_parts(&assignment);
        let Some(e) = else_body(&els) else { return };
        e.is_begin()
    } else {
        let (_, if_branch, els) = parts(&assignment);
        let else_begin = match &els {
            None | Some(Else::Body(None)) => return,
            Some(Else::Elsif(_)) => false,
            Some(Else::Body(Some(b))) => b.is_begin(),
        };
        if_branch.as_ref().is_some_and(Body::is_begin) || else_begin
    };
    if single_line_only(cx) && any_begin {
        return;
    }
    let edits = move_inside(src, node, &rhs);
    report(cx, r, ASSIGN_TO_CONDITION_MSG, edits);
}

/// `node.children.first` of a conditional or parenthesised node.
fn first_child<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    match kind::of(n) {
        kind::PARENTHESES_NODE => {
            let body = n.as_parentheses_node().unwrap().body()?;
            match body.as_statements_node() {
                Some(s) => s.body().iter().next(),
                None => Some(body),
            }
        }
        kind::IF_NODE | kind::UNLESS_NODE => Some(parts(n).0),
        kind::CASE_NODE => n.as_case_node().unwrap().predicate(),
        kind::CASE_MATCH_NODE => n.as_case_match_node().unwrap().predicate(),
        _ => None,
    }
}

/// move_assignment_inside_condition; None where Ruby raises.
fn move_inside(src: &Source, node: &Node, condition: &Node) -> Option<Vec<Edit>> {
    let mut edits = vec![];
    let (start, _) = range(node);
    let cond_start = condition.location().start_offset();
    let assignment = src.slice(start, cond_start).to_string();
    let column = src.column(start);
    let ternary = is_ternary(condition) || first_child(condition).is_some_and(|c| is_ternary(&c));
    let k = kind::of(condition);
    if ternary {
        // ponytail: only an `if` target is handled; a case whose subject is a ternary raises here.
        let t = match one_child(condition) {
            Some(inner) => inner.as_if_node()?,
            None => condition.as_if_node()?,
        };
        let if_b = t.statements().and_then(|s| s.body().iter().next())?;
        let else_b = t.subsequent()?.as_else_node()?.statements().and_then(|s| s.body().iter().next())?;
        if let Some(p) = condition.as_parentheses_node() {
            let (o, c) = (p.opening_loc(), p.closing_loc());
            edits.push(Edit::Remove(o.start_offset(), o.end_offset()));
            edits.push(Edit::Remove(c.start_offset(), c.end_offset()));
        }
        edits.push(Edit::Remove(start, cond_start));
        let (s, e) = range(&if_b);
        edits.push(Edit::InsertBefore(s, e, assignment.clone()));
        let (s, e) = range(&else_b);
        edits.push(Edit::InsertBefore(s, e, assignment));
        return Some(edits);
    }
    if k == kind::CASE_NODE || k == kind::CASE_MATCH_NODE {
        edits.push(Edit::Remove(start, cond_start));
        let (whens, els, end_kw) = case_parts(condition);
        let else_kw = els.as_ref().map(|e| e.else_keyword_loc().start_offset());
        let eb = else_body(&els);
        let eb_start = eb.as_ref().map(|b| b.range().0);
        let case_kw = cond_start;
        let mut all: Vec<(Option<Body>, usize)> = whens;
        // else? adds the else body (nil when empty) to the branches.
        if els.is_some() {
            all.push((eb, case_kw));
        }
        let cond_else = else_kw.map(|k| (k, eb_start));
        let cond_end = Some((end_kw, eb_start));
        for (branch, keyword) in &all {
            let branch = branch.as_ref()?;
            let t = branch.tail()?;
            let (s, e) = range(&t);
            edits.push(Edit::InsertBefore(s, e, assignment.clone()));
            remove_whitespace_in_branches(src, &mut edits, branch, column, cond_else, cond_start, cond_end)?;
            if src.line_of(*keyword) != src.line_of(cond_start) {
                remove_preceding(src, &mut edits, *keyword, column)?;
            }
        }
        return Some(edits);
    }
    if k != kind::IF_NODE && k != kind::UNLESS_NODE {
        return Some(edits);
    }
    edits.push(Edit::Remove(start, cond_start));
    let mut branches = vec![];
    if_branches(condition, &mut branches);
    let (_, _, els) = parts(condition);
    let else_start = match &els {
        Some(Else::Elsif(n)) => Some(n.location().start_offset()),
        Some(Else::Body(b)) => b.as_ref().map(|b| b.range().0),
        None => None,
    };
    let cond_else = loc_else(condition).map(|k| (k, else_start));
    // branches.last.parent.else_branch: the else body of the innermost if of the chain.
    let mut last = parts(condition).2;
    let last_else_start = loop {
        match last {
            Some(Else::Elsif(n)) => last = parts(&n).2,
            Some(Else::Body(b)) => break b.map(|b| b.range().0),
            None => break None,
        }
    };
    let cond_end = end_keyword(condition).map(|e| (e, last_else_start));
    for b in &branches {
        let body = b.body.as_ref()?;
        let t = body.tail()?;
        let (s, e) = range(&t);
        edits.push(Edit::InsertBefore(s, e, assignment.clone()));
        remove_whitespace_in_branches(src, &mut edits, body, column, cond_else, cond_start, cond_end)?;
        if let Some(be) = b.parent_else
            && src.line_of(be) != src.line_of(cond_start)
        {
            remove_preceding(src, &mut edits, be, column)?;
        }
    }
    Some(edits)
}

/// The only child of a parser `begin` from parentheses (`begin_type? && children.one?`).
fn one_child<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    let body = n.as_parentheses_node()?.body()?;
    match body.as_statements_node() {
        Some(s) if s.body().iter().count() == 1 => s.body().iter().next(),
        Some(_) => None,
        None => Some(body),
    }
}

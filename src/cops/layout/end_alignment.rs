use crate::prelude::*;
use helpers::check_assignment::{KINDS, extract_rhs, first_part_of_call_chain};
use helpers::end_keyword_alignment::{detect, misalignment, start_line_range};
use ruby_prism::Location;
use std::collections::{HashMap, HashSet};

pub const NAME: &str = "Layout/EndAlignment";
pub static COP: EndAlignment = EndAlignment;
pub struct EndAlignment;

/// `ignore_node`d conditionals (checked through their assignment), by span, and the offense their
/// assignment check found, emitted when the node itself is visited (its ancestors are needed for
/// the `[Correctable]` flag).
#[derive(Default)]
struct Ignored(HashSet<(usize, usize)>, HashMap<(usize, usize), ((usize, usize), String)>);

fn span(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

fn lspan(l: Location) -> (usize, usize) {
    (l.start_offset(), l.end_offset())
}

/// `(loc.keyword, loc.end)` of a node EndAlignment handles; None for nodes it skips.
type Span = (usize, usize);

fn keyword_and_end(node: &Node) -> Option<(Span, Option<Span>)> {
    Some(match kind::of(node) {
        kind::CLASS_NODE => {
            let n = node.as_class_node().unwrap();
            (lspan(n.class_keyword_loc()), Some(lspan(n.end_keyword_loc())))
        }
        kind::MODULE_NODE => {
            let n = node.as_module_node().unwrap();
            (lspan(n.module_keyword_loc()), Some(lspan(n.end_keyword_loc())))
        }
        kind::SINGLETON_CLASS_NODE => {
            let n = node.as_singleton_class_node().unwrap();
            (lspan(n.class_keyword_loc()), Some(lspan(n.end_keyword_loc())))
        }
        kind::IF_NODE => {
            let n = node.as_if_node().unwrap();
            let kw = n.if_keyword_loc()?; // ternary
            if kw.as_slice() == b"elsif" {
                return None;
            }
            (lspan(kw), n.end_keyword_loc().map(lspan))
        }
        kind::UNLESS_NODE => {
            let n = node.as_unless_node().unwrap();
            (lspan(n.keyword_loc()), n.end_keyword_loc().map(lspan))
        }
        kind::WHILE_NODE => {
            let n = node.as_while_node().unwrap();
            if n.is_begin_modifier() {
                return None;
            }
            (lspan(n.keyword_loc()), n.closing_loc().map(lspan))
        }
        kind::UNTIL_NODE => {
            let n = node.as_until_node().unwrap();
            if n.is_begin_modifier() {
                return None;
            }
            (lspan(n.keyword_loc()), n.closing_loc().map(lspan))
        }
        kind::CASE_NODE => {
            let n = node.as_case_node().unwrap();
            (lspan(n.case_keyword_loc()), Some(lspan(n.end_keyword_loc())))
        }
        kind::CASE_MATCH_NODE => {
            let n = node.as_case_match_node().unwrap();
            (lspan(n.case_keyword_loc()), Some(lspan(n.end_keyword_loc())))
        }
        _ => return None,
    })
}

fn style(cx: &Cx) -> String {
    cx.cfg.str("EnforcedStyleAlignWith").unwrap_or("keyword").to_string()
}

/// `check_other_alignment(node)`.
fn check_other(cx: &mut Cx, node: &Node) {
    let Some((kw, end)) = keyword_and_end(node) else { return };
    if cx.state::<Ignored>().0.contains(&span(node)) {
        // Checked through its assignment: report what that check found, now that the node's
        // ancestors are known.
        if let Some((end, msg)) = cx.state::<Ignored>().1.remove(&span(node)) {
            emit(cx, node, end, msg);
        }
        return;
    }
    let Some(end) = end else { return };
    let sol = start_line_range(cx.src, span(node).0);
    let style = style(cx);
    let ranges = [("keyword", kw), ("variable", kw), ("start_of_line", sol)];
    detect(cx, end, &ranges, &style);
    if let Some(msg) = misalignment(cx.src, end, &ranges, &style) {
        emit(cx, node, end, msg);
    }
}

/// `check_asgn_alignment(outer_node, inner_node)`: the offense is emitted when the inner node is
/// visited (see `Ignored`).
fn check_asgn(cx: &mut Cx, outer_start: usize, inner: &Node) {
    let Some((kw, end)) = keyword_and_end(inner) else { return };
    let ignored = !cx.state::<Ignored>().0.insert(span(inner));
    let Some(end) = end else { return };
    if ignored {
        return;
    }
    let src = cx.src;
    let variable =
        if src.line_of(span(inner).0) > src.line_of(outer_start) { kw } else { (outer_start, kw.1) };
    let sol = start_line_range(src, span(inner).0);
    let style = style(cx);
    let ranges = [("keyword", kw), ("start_of_line", sol), ("variable", variable)];
    detect(cx, end, &ranges, &style);
    if let Some(msg) = misalignment(src, end, &ranges, &style) {
        cx.state::<Ignored>().1.insert(span(inner), (end, msg));
    }
}

/// Report at `end`, autocorrecting with `AlignmentCorrector.align_end(.., alignment_node(node))`.
fn emit(cx: &mut Cx, node: &Node, end: (usize, usize), msg: String) {
    let src = cx.src;
    let column = match style(cx).as_str() {
        "keyword" => src.column(span(node).0),
        "variable" => variable_alignment_column(cx, node),
        _ => src.column(start_line_range(src, span(node).0).0),
    };
    let tabs = helpers::alignment_corrector::Settings::of(cx).tabs;
    cx.add_fix(end.0, end.1, msg, |c| helpers::alignment_corrector::align_end(c, src, tabs, end.0, column));
}

/// Column of `alignment_node(node)` in variable style (the node's ancestors are `cx.ancestors`).
fn variable_alignment_column(cx: &Cx, node: &Node) -> usize {
    let src = cx.src;
    let line = |off: usize| src.line_of(off);
    let anc = parser_ancestor_ranges(cx.ancestors);
    let start = span(node).0;
    let case_arg = matches!(kind::of(node), kind::CASE_NODE | kind::CASE_MATCH_NODE)
        && argument_parent(cx).is_some()
        && anc.first().is_some_and(|&(_, s, _)| line(s) == line(start));
    if case_arg {
        return src.column(anc[0].1);
    }
    // assignment_or_operator_method (1.92): the innermost ancestor whose leading node is (or, for
    // an operator method, is not) the node.
    let is_node = |n: Option<Node>| n.is_some_and(|n| kind::of(&n) == kind::of(node) && span(&n) == span(node));
    let found = anc.iter().find(|&&(i, _, _)| {
        let a = &cx.ancestors[i];
        let send = a.as_call_node().filter(|c| !is_csend(c));
        if helpers::node_types::is_assignment(a) || send.as_ref().is_some_and(|c| method_name(c) == "<<") {
            is_node(extract_rhs(a, true).and_then(leading_node))
        } else if let Some(c) = send {
            if helpers::node_types::is_operator_method(method_name(&c)) {
                !is_node(leading_node(c.as_node()))
            } else {
                is_node(send_args(&c).pop().and_then(leading_node))
            }
        } else {
            false
        }
    });
    // line_break_before_keyword?: fall back to the node itself.
    match found {
        Some(&(_, s, _)) if line(start) <= line(s) => src.column(s),
        _ => src.column(start),
    }
}

/// leading_node: the first part of the call chain, unwrapped from `begin`/`or`/`and` to its
/// first child.
fn leading_node(rhs: Node) -> Option<Node> {
    let mut rhs = first_part_of_call_chain(rhs)?;
    loop {
        rhs = match kind::of(&rhs) {
            kind::PARENTHESES_NODE => rhs.as_parentheses_node().unwrap().body().and_then(|b| match b.as_statements_node() {
                Some(s) => s.body().iter().next(),
                None => Some(b),
            })?,
            kind::AND_NODE => rhs.as_and_node().unwrap().left(),
            kind::OR_NODE => rhs.as_or_node().unwrap().left(),
            _ => return Some(rhs),
        };
    }
}

fn is_conditional(node: &Node) -> bool {
    match kind::of(node) {
        kind::IF_NODE => node.as_if_node().unwrap().if_keyword_loc().is_some(),
        kind::WHILE_NODE => !node.as_while_node().unwrap().is_begin_modifier(),
        kind::UNTIL_NODE => !node.as_until_node().unwrap().is_begin_modifier(),
        kind::UNLESS_NODE | kind::CASE_NODE | kind::CASE_MATCH_NODE => true,
        _ => false,
    }
}

/// The parser parent of a node that is an assignment (setter calls included), if it is one.
fn assignment_parent<'a, 'pr>(cx: &'a Cx<'_, 'pr>) -> Option<&'a Node<'pr>> {
    let n = cx.ancestors.len();
    let parent = cx.ancestors.last()?;
    if kind::of(parent) == kind::ARGUMENTS_NODE {
        let gp = cx.ancestors.get(n.checked_sub(2)?)?;
        return gp.as_call_node().is_some_and(|c| c.equal_loc().is_some()).then_some(gp);
    }
    (kind::of(parent) != kind::CALL_NODE && KINDS.contains(&kind::of(parent))).then_some(parent)
}

/// `node.argument?`: an argument of a (non-`&.`) send.
fn argument_parent<'a, 'pr>(cx: &'a Cx<'_, 'pr>) -> Option<&'a Node<'pr>> {
    let n = cx.ancestors.len();
    if kind::of(cx.ancestors.last()?) != kind::ARGUMENTS_NODE {
        return None;
    }
    let gp = cx.ancestors.get(n.checked_sub(2)?)?;
    gp.as_call_node().is_some_and(|c| !is_csend(&c)).then_some(gp)
}

impl Cop for EndAlignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        static K: LazyLock<Vec<usize>> = LazyLock::new(|| {
            let mut k = KINDS.to_vec();
            k.extend([
                kind::CLASS_NODE,
                kind::SINGLETON_CLASS_NODE,
                kind::MODULE_NODE,
                kind::IF_NODE,
                kind::UNLESS_NODE,
                kind::WHILE_NODE,
                kind::UNTIL_NODE,
                kind::CASE_NODE,
                kind::CASE_MATCH_NODE,
            ]);
            k
        });
        &K
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match kind::of(node) {
            // `on_class` / `on_module` alias `on_sclass` (1.92.2).
            kind::SINGLETON_CLASS_NODE | kind::CLASS_NODE | kind::MODULE_NODE => match assignment_parent(cx) {
                Some(p) => {
                    let start = span(p).0;
                    check_asgn(cx, start, node);
                    check_other(cx, node);
                }
                None => check_other(cx, node),
            },
            kind::CASE_NODE | kind::CASE_MATCH_NODE => match argument_parent(cx) {
                Some(p) => {
                    let start = send_range(&p.as_call_node().unwrap()).0;
                    check_asgn(cx, start, node);
                    check_other(cx, node);
                }
                None => check_other(cx, node),
            },
            kind::IF_NODE
            | kind::UNLESS_NODE
            | kind::WHILE_NODE
            | kind::UNTIL_NODE => {
                if is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                check_other(cx, node);
            }
            _ => {
                // CheckAssignment: on_*asgn / on_send (not csend).
                let Some(rhs) = extract_rhs(node, false).and_then(leading_node) else { return };
                if !is_conditional(&rhs) {
                    return;
                }
                check_asgn(cx, span(node).0, &rhs);
            }
        }
    }
}

use crate::prelude::*;
use helpers::check_assignment::{KINDS, extract_rhs, first_part_of_call_chain};
use helpers::end_keyword_alignment::{begins_its_line, column_offset_between, start_line_range};
use ruby_prism::Location;
use std::collections::HashSet;

pub const NAME: &str = "Layout/ElseAlignment";
pub static COP: ElseAlignment = ElseAlignment;
pub struct ElseAlignment;

/// `ignore_node`d if nodes (checked through an assignment or an enclosing if), by span.
#[derive(Default)]
struct Ignored(HashSet<(usize, usize)>);

fn span(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

fn lspan(l: Location) -> (usize, usize) {
    (l.start_offset(), l.end_offset())
}

/// `node.loc.else` of an if/unless (an `elsif` keyword, `else`, or a ternary's `:`), and the
/// else branch when it is an `elsif`.
fn else_parts<'pr>(node: &Node<'pr>) -> Option<((usize, usize), Option<Node<'pr>>)> {
    if let Some(n) = node.as_if_node() {
        n.if_keyword_loc()?; // a ternary's `:` is not `loc.else`
        let sub = n.subsequent()?;
        if let Some(elsif) = sub.as_if_node() {
            return Some((lspan(elsif.if_keyword_loc()?), Some(sub)));
        }
        return Some((lspan(sub.as_else_node()?.else_keyword_loc()), None));
    }
    let e = node.as_unless_node()?.else_clause()?;
    Some((lspan(e.else_keyword_loc()), None))
}

/// rubocop-ast `if?` / `unless?`: an if/unless with keyword `if`/`unless` (modifiers included).
fn if_or_unless(node: &Node) -> bool {
    match node.as_if_node() {
        Some(n) => n.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if"),
        None => node.as_unless_node().is_some(),
    }
}

fn check_alignment(cx: &mut Cx, base: (usize, usize), else_range: (usize, usize)) {
    let src = cx.src;
    if !begins_its_line(src, else_range.0) {
        return;
    }
    let delta = column_offset_between(src, base.0, else_range.0);
    if delta == 0 {
        return;
    }
    let base_src = src.slice(base.0, base.1);
    let word = &base_src[..base_src.find(char::is_whitespace).unwrap_or(base_src.len())];
    let msg = format!("Align `{}` with `{}`.", src.slice(else_range.0, else_range.1), word);
    let set = helpers::alignment_corrector::Settings::of(cx);
    let (s, e) = else_range;
    cx.add_fix(s, e, msg, |c| helpers::alignment_corrector::correct(c, src, set, s, e, None, delta, false));
}

/// `on_if(node, base)`; `ancestors` are the node's Prism ancestors (for `base_range_of_if`).
fn on_if(cx: &mut Cx, node: &Node, ancestors: &[Node], base: Option<(usize, usize)>) {
    if cx.state::<Ignored>().0.contains(&span(node)) {
        return;
    }
    let Some((else_range, elsif)) = else_parts(node) else { return };
    if !begins_its_line(cx.src, else_range.0) {
        return;
    }
    let base = match base {
        Some(b) => b,
        None => {
            let keyword = |f: &Node| match f.as_if_node() {
                Some(n) => lspan(n.if_keyword_loc().unwrap()),
                None => lspan(f.as_unless_node().unwrap().keyword_loc()),
            };
            let found = if if_or_unless(node) {
                Some(keyword(node))
            } else {
                ancestors
                    .iter()
                    .rev()
                    .filter(|a| matches!(kind::of(a), kind::IF_NODE | kind::UNLESS_NODE))
                    .find(|a| if_or_unless(a))
                    .map(keyword)
            };
            // RuboCop raises when no `if`/`unless` encloses the node.
            let Some(f) = found else { return };
            f
        }
    };
    check_alignment(cx, base, else_range);
    if let Some(elsif) = elsif {
        on_if(cx, &elsif, &[], Some(base));
        cx.state::<Ignored>().0.insert(span(&elsif));
    }
}

/// `base_range_of_rescue`: what a `rescue ... else` aligns with, by the parser parent.
fn rescue_base(cx: &Cx, begin: &ruby_prism::BeginNode) -> Option<(usize, usize)> {
    if let Some(b) = begin.begin_keyword_loc() {
        return Some(lspan(b));
    }
    let n = cx.ancestors.len();
    let parent = cx.ancestors.last()?;
    Some(match kind::of(parent) {
        kind::DEF_NODE => {
            let d = parent.as_def_node().unwrap();
            // base_for_method_definition: `private def ...` aligns with the modifier's selector.
            let grand = n.checked_sub(2).map(|i| &cx.ancestors[i]);
            let send = match grand {
                Some(g) if kind::of(g) == kind::ARGUMENTS_NODE => n.checked_sub(3).map(|i| &cx.ancestors[i]),
                Some(g) if kind::of(g) == kind::CALL_NODE => Some(g),
                _ => None,
            }
            .and_then(|s| s.as_call_node())
            .filter(|c| !is_csend(c));
            match send.and_then(|c| c.message_loc()) {
                Some(sel) => lspan(sel),
                None => lspan(d.def_keyword_loc()),
            }
        }
        kind::BLOCK_NODE => {
            let start = n.checked_sub(2).map_or(span(parent).0, |i| span(&cx.ancestors[i]).0);
            start_line_range(cx.src, start)
        }
        kind::LAMBDA_NODE => start_line_range(cx.src, span(parent).0),
        kind::CLASS_NODE => lspan(parent.as_class_node().unwrap().class_keyword_loc()),
        kind::MODULE_NODE => lspan(parent.as_module_node().unwrap().module_keyword_loc()),
        kind::SINGLETON_CLASS_NODE => lspan(parent.as_singleton_class_node().unwrap().class_keyword_loc()),
        _ => return None,
    })
}

impl Cop for ElseAlignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        static K: LazyLock<Vec<usize>> = LazyLock::new(|| {
            let mut k = KINDS.to_vec();
            k.extend([kind::IF_NODE, kind::UNLESS_NODE, kind::BEGIN_NODE, kind::CASE_NODE, kind::CASE_MATCH_NODE]);
            k
        });
        &K
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match kind::of(node) {
            kind::IF_NODE | kind::UNLESS_NODE => {
                if is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                on_if(cx, node, cx.ancestors, None);
            }
            kind::BEGIN_NODE => {
                let b = node.as_begin_node().unwrap();
                let (Some(_), Some(e)) = (b.rescue_clause(), b.else_clause()) else { return };
                let Some(base) = rescue_base(cx, &b) else { return };
                check_alignment(cx, base, lspan(e.else_keyword_loc()));
            }
            kind::CASE_NODE => {
                let c = node.as_case_node().unwrap();
                let (Some(e), Some(last)) = (c.else_clause(), c.conditions().iter().last()) else { return };
                let Some(w) = last.as_when_node() else { return };
                check_alignment(cx, lspan(w.keyword_loc()), lspan(e.else_keyword_loc()));
            }
            kind::CASE_MATCH_NODE => {
                let c = node.as_case_match_node().unwrap();
                let (Some(e), Some(last)) = (c.else_clause(), c.conditions().iter().last()) else { return };
                let Some(i) = last.as_in_node() else { return };
                check_alignment(cx, lspan(i.in_loc()), lspan(e.else_keyword_loc()));
            }
            _ => {
                // CheckAssignment (on_*asgn, on_send).
                let Some(rhs) = extract_rhs(node, false).and_then(first_part_of_call_chain) else { return };
                if !matches!(kind::of(&rhs), kind::IF_NODE | kind::UNLESS_NODE) {
                    return;
                }
                let src = cx.src;
                let end_cfg = cx.config.for_cop("Layout/EndAlignment");
                let end_style = end_cfg.str("EnforcedStyleAlignWith").unwrap_or("keyword");
                let node_range = match node.as_call_node() {
                    Some(c) => send_range(&c),
                    None => span(node),
                };
                let variable = end_style != "keyword" && src.line_of(span(&rhs).0) <= src.line_of(node_range.0);
                let base = if variable { node_range } else { span(&rhs) };
                // The rhs's Prism ancestors are not needed: a base is given.
                on_if(cx, &rhs, &[], Some(base));
                cx.state::<Ignored>().0.insert(span(&rhs));
            }
        }
    }
}

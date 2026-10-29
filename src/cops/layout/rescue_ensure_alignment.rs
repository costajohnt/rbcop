use crate::helpers::macro_scope::in_macro_scope_of_child as in_macro_scope;
use crate::prelude::*;
use crate::source::Source;

pub const NAME: &str = "Layout/RescueEnsureAlignment";
pub static COP: RescueEnsureAlignment = RescueEnsureAlignment;
pub struct RescueEnsureAlignment;

fn span(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// Offset of the first non-blank character on the line holding `off` (`line =~ /\S/`).
fn first_non_space(src: &Source, off: usize) -> usize {
    let s = src.line_start(src.line_of(off));
    s + src.text[s..].bytes().take_while(|b| matches!(b, b' ' | b'\t' | b'\x0b' | b'\x0c' | b'\r')).count()
}

/// End of `node.loc.name` (or `node.lhs` for masgn and op-assignments) for an assignment node.
fn assignment_name_end(n: &Node) -> Option<Option<usize>> {
    macro_rules! name {
        ($($as:ident),*) => {
            $(if let Some(x) = n.$as() { return Some(Some(x.name_loc().end_offset())); })*
        };
    }
    name!(
        as_local_variable_write_node,
        as_instance_variable_write_node,
        as_class_variable_write_node,
        as_global_variable_write_node,
        as_constant_write_node,
        as_local_variable_operator_write_node,
        as_instance_variable_operator_write_node,
        as_class_variable_operator_write_node,
        as_global_variable_operator_write_node,
        as_constant_operator_write_node,
        as_local_variable_or_write_node,
        as_instance_variable_or_write_node,
        as_class_variable_or_write_node,
        as_global_variable_or_write_node,
        as_constant_or_write_node,
        as_local_variable_and_write_node,
        as_instance_variable_and_write_node,
        as_class_variable_and_write_node,
        as_global_variable_and_write_node,
        as_constant_and_write_node
    );
    macro_rules! target {
        ($($as:ident),*) => {
            $(if let Some(x) = n.$as() { return Some(Some(x.target().name_loc().end_offset())); })*
        };
    }
    target!(
        as_constant_path_write_node,
        as_constant_path_operator_write_node,
        as_constant_path_or_write_node,
        as_constant_path_and_write_node
    );
    if let Some(m) = n.as_multi_write_node() {
        // masgn: `node.lhs.source_range`
        let end = m.rparen_loc().map(|l| l.end_offset()).or_else(|| {
            let mut ends: Vec<usize> = m.lefts().iter().chain(m.rights().iter()).map(|t| span(&t).1).collect();
            ends.extend(crate::ast::parser_rest(m.rest()).map(|r| span(&r).1));
            ends.into_iter().max()
        });
        return Some(end);
    }
    // op_asgn / or_asgn / and_asgn on a send: `node.lhs.source_range` (1.92.2), the `a.b` or
    // `a[i]` target.
    macro_rules! call_target {
        ($($as:ident),*) => {
            $(if let Some(x) = n.$as() { return Some(x.message_loc().map(|l| l.end_offset())); })*
        };
    }
    call_target!(as_call_operator_write_node, as_call_or_write_node, as_call_and_write_node);
    macro_rules! index_target {
        ($($as:ident),*) => {
            $(if let Some(x) = n.$as() { return Some(Some(x.closing_loc().end_offset())); })*
        };
    }
    index_target!(as_index_operator_write_node, as_index_or_write_node, as_index_and_write_node);
    None
}

impl Cop for RescueEnsureAlignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RESCUE_NODE, kind::ENSURE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let kw = match node.as_rescue_node() {
            Some(r) => r.keyword_loc(),
            None => node.as_ensure_node().unwrap().ensure_keyword_loc(),
        };
        let src = cx.src;
        let anc = cx.ancestors;
        let is_anchor = |n: &Node| match kind::of(n) {
            kind::BEGIN_NODE => n.as_begin_node().unwrap().begin_keyword_loc().is_some(),
            kind::DEF_NODE
            | kind::CLASS_NODE
            | kind::MODULE_NODE
            | kind::SINGLETON_CLASS_NODE
            | kind::BLOCK_NODE
            | kind::LAMBDA_NODE => true,
            _ => false,
        };
        let Some(j) = (0..anc.len()).rev().find(|&i| is_anchor(&anc[i])) else { return };
        let a = &anc[j];
        let kw_line = src.line_of(kw.start_offset());
        let kw_col = src.column(kw.start_offset());

        // The ancestor's parser range start, the end of its `ending_loc`, the last line of its
        // send node (blocks) and the Prism index of its parser parent.
        let (start, ending, send_last_line, parent_idx) = match kind::of(a) {
            kind::BEGIN_NODE => {
                let b = a.as_begin_node().unwrap().begin_keyword_loc().unwrap();
                (b.start_offset(), b.end_offset(), None, None)
            }
            kind::BLOCK_NODE | kind::LAMBDA_NODE => {
                let (opening, send_start, send_end, dot, selector, call_idx) = if let Some(l) = a.as_lambda_node() {
                    let op = l.operator_loc();
                    (l.opening_loc(), op.start_offset(), op.end_offset(), None, Some(op.start_offset()), j)
                } else {
                    let opening = a.as_block_node().unwrap().opening_loc();
                    let Some(owner) = j.checked_sub(1).map(|i| &anc[i]) else { return };
                    if let Some(c) = owner.as_call_node() {
                        let (s, e) = send_range(&c);
                        let dot = c.call_operator_loc().map(|l| l.start_offset());
                        (opening, s, e, dot, c.message_loc().map(|l| l.start_offset()), j - 1)
                    } else if let Some(s) = owner.as_super_node() {
                        let e = s
                            .rparen_loc()
                            .map(|l| l.end_offset())
                            .or_else(|| s.arguments().map(|x| x.location().end_offset()))
                            .unwrap_or(s.keyword_loc().end_offset());
                        (opening, span(owner).0, e, None, Some(span(owner).0), j - 1)
                    } else {
                        let s = span(owner).0;
                        (opening, s, s + 5, None, Some(s), j - 1)
                    }
                };
                // aligned_with_line_break_method?
                let do_line = src.line_of(opening.start_offset());
                let at = |o: Option<usize>| o.is_some_and(|o| src.line_of(o) == do_line && src.column(o) == kw_col);
                if at(dot) || at(selector) {
                    return;
                }
                let last_line = src.line_of(send_end);
                (send_start, opening.end_offset(), Some(last_line), call_idx.checked_sub(1))
            }
            kind::DEF_NODE => {
                let d = a.as_def_node().unwrap();
                (span(a).0, d.name_loc().end_offset(), None, j.checked_sub(1))
            }
            kind::CLASS_NODE => {
                let c = a.as_class_node().unwrap();
                (span(a).0, span(&c.constant_path()).1, None, j.checked_sub(1))
            }
            kind::MODULE_NODE => {
                let m = a.as_module_node().unwrap();
                (span(a).0, span(&m.constant_path()).1, None, j.checked_sub(1))
            }
            _ => {
                let s = a.as_singleton_class_node().unwrap();
                (span(a).0, span(&s.expression()).1, None, j.checked_sub(1))
            }
        };

        // assignment_node / access_modifier_node replace the ancestor as alignment node.
        let (mut start, mut ending, mut send_last_line) = (start, ending, send_last_line);
        if kind::of(a) != kind::BEGIN_NODE
            && let Some(p) = parent_idx
        {
            // Parser parent: a call holding us as an argument has no ArgumentsNode in between.
            let p = if kind::of(&anc[p]) == kind::ARGUMENTS_NODE && p >= 1 { p - 1 } else { p };
            let parent = &anc[p];
            let call = parent.as_call_node();
            // `receiver&.source_range || child_nodes.first.loc.name` for sends.
            let call_end = |c: &ruby_prism::CallNode| match c.receiver() {
                Some(r) => Some(span(&r).1),
                None => call_args(c).first().and_then(|f| f.as_def_node()).map(|d| d.name_loc().end_offset()),
            };
            let assignment = match &call {
                Some(c) if c.equal_loc().is_some() => Some(call_end(c)),
                Some(_) => None,
                None => assignment_name_end(parent),
            };
            if let Some(name_end) = assignment {
                if src.line_of(span(parent).0) == src.line_of(start) {
                    let Some(name_end) = name_end else { return }; // RuboCop crashes here
                    (start, ending, send_last_line) = (span(parent).0, name_end, None);
                }
            } else if kind::of(a) == kind::DEF_NODE
                && let Some(c) = call
            {
                let name = method_name(&c);
                let modifier = matches!(name, "public_class_method" | "private_class_method")
                    || (matches!(name, "public" | "protected" | "private" | "module_function")
                        && c.receiver().is_none()
                        && in_macro_scope(&anc[..p]));
                if modifier {
                    let Some(end) = call_end(&c) else { return }; // RuboCop crashes here
                    (start, ending, send_last_line) = (span(parent).0, end, None);
                }
            }
        }

        let bea = cx.config.for_cop("Layout/BeginEndAlignment");
        let align = if bea.truthy("Enabled") && bea.str("EnforcedStyleAlignWith") == Some("start_of_line") {
            first_non_space(src, start)
        } else if let Some(line) = send_last_line {
            first_non_space(src, src.line_start(line))
        } else {
            start
        };
        let (align_line, align_col) = (src.line_of(align), src.column(align));
        if align_col == kw_col || align_line == kw_line {
            return;
        }
        let beginning = src.slice(align, ending.max(align));
        let msg = format!(
            "`{}` at {}, {} is not aligned with `{}` at {}, {}.",
            text(&kw),
            kw_line,
            kw_col,
            beginning,
            align_line,
            align_col
        );
        // autocorrect: replace the text before the keyword with `align_col` spaces, unless some
        // code sits there (`whitespace.source.strip.empty?`).
        let line_start = src.line_start(kw_line);
        let blank = src.slice(line_start, kw.start_offset()).bytes().all(|b| b.is_ascii_whitespace() || b == 0 || b == 0x0b);
        // alignment_indentation: under IndentationStyle tabs, reuse the target line's own
        // indentation, or nothing when the target does not begin its line.
        let pad = if cx.config.for_cop("Layout/IndentationStyle").str("EnforcedStyle") != Some("tabs") {
            Some(" ".repeat(align_col))
        } else if src.begins_its_line(align) {
            let s = src.line_start(align_line);
            Some(src.slice(s, align).to_string())
        } else {
            None
        };
        cx.add_fix(kw.start_offset(), kw.end_offset(), msg, |c| {
            if blank && let Some(pad) = &pad {
                c.replace(line_start, kw.start_offset(), pad);
            }
        });
    }
}

use crate::prelude::*;
use ruby_prism::Location;

pub const NAME: &str = "Layout/CaseIndentation";
pub static COP: CaseIndentation = CaseIndentation;
pub struct CaseIndentation;

impl Cop for CaseIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CASE_NODE, kind::CASE_MATCH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (case keyword, end keyword, else keyword, branches as (keyword, then keyword))
        let (case_kw, end_kw, else_kw, branches, branch_type): (Location, Location, Option<Location>, Vec<_>, &str) =
            if let Some(c) = node.as_case_node() {
                let b = c
                    .conditions()
                    .iter()
                    .filter_map(|w| w.as_when_node())
                    .map(|w| (w.keyword_loc(), w.then_keyword_loc()))
                    .collect();
                (c.case_keyword_loc(), c.end_keyword_loc(), c.else_clause().map(|e| e.else_keyword_loc()), b, "when")
            } else {
                let c = node.as_case_match_node().unwrap();
                let b = c
                    .conditions()
                    .iter()
                    .filter_map(|w| w.as_in_node())
                    .map(|w| (w.in_loc(), w.then_loc()))
                    .collect();
                (c.case_keyword_loc(), c.end_keyword_loc(), c.else_clause().map(|e| e.else_keyword_loc()), b, "in")
            };
        let src = cx.src;
        let line = |l: &Location| src.line_of(l.start_offset());
        if line(&case_kw) == line(&end_kw) {
            return;
        }
        let end_style = cx.cfg.str("EnforcedStyle") == Some("end");
        if end_style {
            let last_conditional = match &else_kw {
                Some(e) => Some(line(e)),
                None => branches.last().and_then(|(_, then)| then.as_ref().map(line)),
            };
            if last_conditional == Some(line(&end_kw)) {
                return;
            }
        }
        let one_step = cx.cfg.bool("IndentOneStep").unwrap_or(false);
        // Under tabs one step is a single tab character, one column (1.92).
        let tabs = helpers::alignment::using_tabs(cx);
        let width = match (one_step, tabs) {
            (false, _) => 0,
            (true, true) => 1,
            (true, false) => helpers::alignment::configured_indentation_width(cx),
        };
        let base_loc = if end_style { end_kw.start_offset() } else { case_kw.start_offset() };
        // replacement: spaces to the base column, or under tabs the base line's indentation
        // plus tabs (nothing when the base does not begin its line).
        let replacement = if !tabs {
            Some(" ".repeat((src.column(base_loc) as i64 + width).max(0) as usize))
        } else if helpers::alignment::begins_its_line(src, base_loc) {
            let ls = src.line_start(src.line_of(base_loc));
            let indent: String = src.text[ls..]
                .chars()
                .take_while(|c| matches!(c, ' ' | '\t' | '\r' | '\x0b' | '\x0c'))
                .collect();
            Some(indent + &"\t".repeat(width.max(0) as usize))
        } else {
            None
        };
        let base = if end_style { src.column(end_kw.start_offset()) } else { src.column(case_kw.start_offset()) } as i64;
        let depth = if one_step { "one step more than" } else { "as deep as" };
        let style = if end_style { "end" } else { "case" };
        for (kw, _) in &branches {
            let column = src.column(kw.start_offset()) as i64;
            if column == base + width {
                cx.correct_style_detected();
                continue;
            }
            let start = kw.start_offset();
            // detect_incorrect_style, in the offense's block
            if cx.block_runs(start, kw.end_offset()) {
                let alternative = if end_style { src.column(case_kw.start_offset()) } else { src.column(end_kw.start_offset()) };
                if column == alternative as i64 {
                    cx.opposite_style_detected();
                } else {
                    cx.no_acceptable_style();
                }
            }
            let ls = src.line_start(src.line_of(start));
            let blank = src.text[ls..start].trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0' || c == '\x0b').is_empty();
            let replacement = replacement.as_deref().filter(|_| blank);
            cx.add_fix(start, kw.end_offset(), format!("Indent `{branch_type}` {depth} `{style}`."), |c| {
                if let Some(r) = replacement {
                    c.replace(ls, start, r);
                }
            });
        }
    }
}

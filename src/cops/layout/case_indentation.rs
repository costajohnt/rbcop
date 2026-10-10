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
        let width = if one_step { helpers::alignment_mixin::indentation_width(cx) } else { 0 };
        let base = if end_style { src.column(end_kw.start_offset()) } else { src.column(case_kw.start_offset()) } as i64;
        let depth = if one_step { "one step more than" } else { "as deep as" };
        let style = if end_style { "end" } else { "case" };
        for (kw, _) in &branches {
            if src.column(kw.start_offset()) as i64 == base + width {
                continue;
            }
            let start = kw.start_offset();
            let fixable = src.begins_its_line(start);
            cx.add_with(start, kw.end_offset(), format!("Indent `{branch_type}` {depth} `{style}`."), fixable);
        }
    }
}

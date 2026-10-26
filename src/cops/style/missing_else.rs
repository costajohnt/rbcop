use crate::prelude::*;
use helpers::if_node::{normal_keyword, parser_end};

pub const NAME: &str = "Style/MissingElse";
pub static COP: MissingElse = MissingElse;
pub struct MissingElse;

impl Cop for MissingElse {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::CASE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("both");
        let (ty, has_else, end_kw, end) = if let Some(case) = node.as_case_node() {
            if style == "if" {
                return;
            }
            ("case", case.else_clause().is_some(), Some(case.end_keyword_loc()), node.location().end_offset())
        } else {
            if is_pattern_guard(node, cx.ancestors) || style == "case" {
                return;
            }
            let Some(kw) = normal_keyword(node) else { return };
            if kw == "unless" {
                let enabled = cx.config.for_cop("Style/UnlessElse").get("Enabled").is_some_and(|v| !matches!(v, Value::Bool(false) | Value::Null));
                if enabled {
                    return;
                }
            }
            let (has_else, end_kw) = match node.as_if_node() {
                Some(n) => (n.subsequent().is_some(), n.end_keyword_loc()),
                None => {
                    let n = node.as_unless_node().unwrap();
                    (n.else_clause().is_some(), n.end_keyword_loc())
                }
            };
            ("if", has_else, end_kw, parser_end(node))
        };
        if has_else {
            return;
        }
        // 1.92: a body that is a guard clause is left to Style/GuardClause (the two would conflict).
        if ty == "if"
            && helpers::alignment::cop_enabled(cx, "Style/GuardClause")
            && helpers::if_node::if_branch(node)
                .is_some_and(|b| helpers::if_node::guard_clause(cx.src, &b))
        {
            return;
        }
        let empty_else = cx.config.for_cop("Style/EmptyElse");
        let empty_else_style = empty_else.str("EnforcedStyle");
        let msg = match empty_else_style {
            Some("empty") => "`{}` condition requires an `else`-clause with `nil` in it.",
            Some("nil") => "`{}` condition requires an empty `else`-clause.",
            _ => "`{}` condition requires an `else`-clause.",
        }
        .replace("{}", ty);
        let insertion = match empty_else_style {
            Some("empty") => Some("else; nil; "),
            Some("nil") => Some("else; "),
            _ => None,
        };
        let end_kw = end_kw.map(|l| (l.start_offset(), l.end_offset()));
        let start = node.location().start_offset();
        cx.add_fix(start, end, msg, |c| {
            if let (Some(text), Some((s, e))) = (insertion, end_kw) {
                c.insert_before(s, e, text);
            }
        });
    }
}

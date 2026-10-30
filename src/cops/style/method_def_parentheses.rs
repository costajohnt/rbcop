use crate::prelude::*;

pub const NAME: &str = "Style/MethodDefParentheses";
pub static COP: MethodDefParentheses = MethodDefParentheses;
pub struct MethodDefParentheses;

impl Cop for MethodDefParentheses {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let params = def.parameters();
        // Parser `args` range: the parentheses when present, else the parameters (none when empty).
        let parens = def.lparen_loc().zip(def.rparen_loc()).map(|(l, r)| (l.start_offset(), r.end_offset()));
        let args = parens.or(params.as_ref().map(|p| (p.location().start_offset(), p.location().end_offset())));
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("require_parentheses");
        let multiline = args.is_some_and(|(s, e)| cx.src.line_of(s) != cx.src.line_of(e));
        if style == "require_parentheses" || (style == "require_no_parentheses_except_multiline" && multiline) {
            if parens.is_none()
                && let Some(p) = &params
            {
                let (s, e) = (p.location().start_offset(), p.location().end_offset());
                // range_before_arguments: from the name to the arguments, cut at the last line
                // break so a `\` line continuation stays out of the `(`.
                let name_end = def.name_loc().end_offset();
                let before = name_end + cx.src.text[name_end..s].rfind('\n').unwrap_or(s - name_end);
                // `unexpected_style_detected 'require_no_parentheses'`: a String, so nil.
                if cx.block_runs(s, e) {
                    cx.string_style_detected_for("EnforcedStyle");
                }
                cx.add_fix(s, e, "Use def with parentheses when there are parameters.", |c| {
                    c.replace(name_end, before, "(");
                    c.insert_after(s, e, ")");
                });
            } else {
                cx.correct_style_detected();
            }
            return;
        }
        // 1.92 also forces them for parameters starting below the `(` or with a `(a, b)` destructuring.
        let first = params.as_ref().and_then(|p| parameter_nodes(p).into_iter().next());
        let own_line =
            parens.zip(first.as_ref()).is_some_and(|((s, _), f)| cx.src.line_of(s) != cx.src.line_of(f.location().start_offset()));
        let destructuring = first.as_ref().is_some_and(|f| f.as_multi_target_node().is_some());
        if def.equal_loc().is_some() || params.as_ref().is_some_and(anonymous_arguments) || own_line || destructuring {
            return cx.correct_style_detected();
        }
        if let Some((s, e)) = parens {
            if cx.block_runs(s, e) {
                cx.string_style_detected_for("EnforcedStyle");
            }
            // code_after_arguments?: `def foo(a) bar end` keeps a `;` in place of the `)`.
            let line_end = cx.src.text[e..].find('\n').map_or(cx.src.text.len(), |i| e + i);
            let rest = cx.src.text[e..line_end].trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0');
            let code_after = !rest.is_empty() && !rest.starts_with([';', '#']);
            cx.add_fix(s, e, "Use def without parentheses.", |c| {
                c.replace(s, s + 1, " ");
                if code_after { c.replace(e - 1, e, ";") } else { c.remove(e - 1, e) }
            });
        }
    }
}

/// `...`, bare `*`, `**` or `&`.
fn anonymous_arguments(p: &ruby_prism::ParametersNode) -> bool {
    p.keyword_rest().is_some_and(|k| {
        k.as_forwarding_parameter_node().is_some() || k.as_keyword_rest_parameter_node().is_some_and(|k| k.name().is_none())
    }) || crate::ast::parser_rest(p.rest()).and_then(|r| r.as_rest_parameter_node()).is_some_and(|r| r.name().is_none())
        || p.block().is_some_and(|b| b.name().is_none())
}

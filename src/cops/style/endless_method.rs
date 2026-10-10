use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/EndlessMethod";
pub static COP: EndlessMethod = EndlessMethod;
pub struct EndlessMethod;

const MSG: &str = "Avoid endless method definitions.";
const MSG_MULTI_LINE: &str = "Avoid endless method definitions with multiple lines.";
const MSG_REQUIRE_SINGLE: &str = "Use endless method definitions for single line methods.";
const MSG_REQUIRE_ALWAYS: &str = "Use endless method definitions.";

/// `each_descendant(:str).any?(&:heredoc?)` (parser `str` only: no interpolation, one body line).
#[derive(Default)]
struct StrHeredoc(bool);

impl<'pr> Visit<'pr> for StrHeredoc {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
        let n = node.as_node();
        self.0 |= is_heredoc(&n) && !is_parser_dstr(&n);
    }
}

/// The parser body of a def: one statement, else the StatementsNode / BeginNode (begin, kwbegin,
/// rescue, ensure).
fn parser_body<'pr>(def: &ruby_prism::DefNode<'pr>) -> Option<(Node<'pr>, bool)> {
    let body = def.body()?;
    if let Some(s) = body.as_statements_node() {
        let mut it = s.body().iter();
        let first = it.next()?;
        if it.next().is_none() {
            let begin = first.as_parentheses_node().is_some() || first.as_begin_node().is_some();
            return Some((first, begin));
        }
    }
    Some((body, true))
}

impl Cop for EndlessMethod {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.0, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let name = std::str::from_utf8(def.name().as_slice()).unwrap_or("");
        // assignment_method?
        if name.ends_with('=') && !matches!(name, "==" | "!=" | "===" | "<=" | ">=") {
            return;
        }
        let body = parser_body(&def);
        if let Some((b, _)) = &body {
            if is_heredoc(b) {
                return;
            }
            let mut v = StrHeredoc::default();
            v.visit(b);
            if v.0 {
                return;
            }
        }
        let src = cx.src;
        let (start, end) = (node.location().start_offset(), node.location().end_offset());
        let endless = def.equal_loc().is_some();
        let multiline = src.line_of(start) != src.line_of(end);
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("");

        // Pieces of `def <receiver><name><arguments>`.
        let receiver = def.receiver().map_or(String::new(), |r| {
            let r = strip_parens(r).or_else(|| def.receiver()).unwrap();
            format!("{}{}", node_text(&r), def.operator_loc().map_or("", |o| text(&o)))
        });
        let has_args = def.parameters().is_some();
        let args_range = def.parameters().map(|p| match (def.lparen_loc(), def.rparen_loc()) {
            (Some(l), Some(r)) => (l.start_offset(), r.end_offset()),
            _ => (p.location().start_offset(), p.location().end_offset()),
        });
        let args = args_range.map_or("", |(s, e)| &src.text[s..e]);
        let body_src = body.as_ref().map_or("", |(b, _)| node_text(b));
        let column = src.column(start);

        let width = cx
            .cfg
            .int("IndentationWidth")
            .or_else(|| cx.config.for_cop("Layout/IndentationWidth").int("Width"))
            .unwrap_or(2) as usize;
        let multiline_fix = format!(
            "def {receiver}{name}{args}\n{}{body_src}\n{}end",
            " ".repeat(column + width),
            " ".repeat(column)
        );
        // endless_parent?
        let endless_parent = helpers::parser_parent::parser_parent(cx.ancestors)
            .and_then(|p| p.as_def_node())
            .is_some_and(|p| p.equal_loc().is_some());
        let to_multiline = |cx: &mut Cx, msg: &str, guard: bool| {
            cx.add_fix(start, end, msg, |c| {
                if !guard {
                    c.replace(start, end, &multiline_fix);
                }
            });
        };

        let can_be_endless = matches!(body, Some((_, false)));
        let replacement = format!("def {receiver}{name}{args} = {body_src}");
        let replacement = replacement.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0');
        let too_long = || {
            helpers::alignment::cop_enabled(cx, "Layout/LineLength") && {
                let max = cx.config.for_cop("Layout/LineLength").int("Max").unwrap_or(120);
                (replacement.chars().count() + column) as i64 > max
            }
        };
        let to_endless = |cx: &mut Cx, msg: &str| {
            let (b, _) = body.as_ref().unwrap();
            let sig_end = if has_args { args_range.unwrap().1 } else { def.name_loc().end_offset() };
            let (bs, be) = (b.location().start_offset(), b.location().end_offset());
            let def_end = def.end_keyword_loc().map_or(end, |e| e.end_offset());
            cx.add_fix(start, end, msg, |c| {
                c.replace(sig_end.min(bs), sig_end.max(bs), " = ");
                c.remove(be.min(def_end), be.max(def_end));
            });
        };

        match style {
            "allow_single_line" | "allow_always" => {
                if endless && multiline && style != "allow_always" {
                    to_multiline(cx, MSG_MULTI_LINE, endless_parent);
                }
            }
            "disallow" => {
                if endless {
                    to_multiline(cx, MSG, endless_parent);
                }
            }
            "require_single_line" => {
                if endless && multiline {
                    to_multiline(cx, MSG_MULTI_LINE, false);
                } else if !endless && can_be_endless && !replacement.contains('\n') && !too_long() {
                    to_endless(cx, MSG_REQUIRE_SINGLE);
                }
            }
            "require_always" if !endless && can_be_endless && !too_long() => {
                to_endless(cx, MSG_REQUIRE_ALWAYS);
            }
            _ => {}
        }
    }
}

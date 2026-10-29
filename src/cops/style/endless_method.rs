use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/EndlessMethod";
pub static COP: EndlessMethod = EndlessMethod;
pub struct EndlessMethod;

const MSG: &str = "Avoid endless method definitions.";
const MSG_MULTI_LINE: &str = "Avoid endless method definitions with multiple lines.";
const MSG_REQUIRE_SINGLE: &str = "Use endless method definitions for single line methods.";
const MSG_REQUIRE_ALWAYS: &str = "Use endless method definitions.";

/// `each_descendant(:any_str).any?(&:heredoc?)`.
#[derive(Default)]
struct StrHeredoc(bool);

impl<'pr> Visit<'pr> for StrHeredoc {
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
        self.0 |= is_heredoc(&node.as_node());
    }
    fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode<'pr>) {
        self.0 |= is_heredoc(&node.as_node());
    }
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
        self.0 |= is_heredoc(&node.as_node());
        ruby_prism::visit_interpolated_string_node(self, node);
    }
    fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode<'pr>) {
        self.0 |= is_heredoc(&node.as_node());
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }
}

/// The parser body of a def: one statement, else the StatementsNode / BeginNode, and whether it
/// is a `begin`, `kwbegin`, `rescue` or `ensure` (`can_be_made_endless?` rejects those).
fn parser_body<'pr>(def: &ruby_prism::DefNode<'pr>) -> Option<(Node<'pr>, bool)> {
    let body = def.body()?;
    if let Some(s) = body.as_statements_node() {
        let mut it = s.body().iter();
        let first = it.next()?;
        if it.next().is_none() {
            // `x rescue y` alone is a parser `rescue` body.
            let begin = first.as_parentheses_node().is_some()
                || first.as_begin_node().is_some()
                || first.as_rescue_modifier_node().is_some();
            return Some((first, begin));
        }
    }
    Some((body, true))
}

/// ambiguous_when_made_endless?: the def is the left operand of an `and`/`or`, or the body of a
/// modifier `if`/`unless`/`while`/`until`, which would then apply to the endless definition's body.
fn ambiguous_when_made_endless(node: &Node, ancestors: &[Node]) -> bool {
    let start = node.location().start_offset();
    let Some(parent) = ancestors.last() else { return false };
    let left = parent.as_and_node().map(|n| n.left()).or_else(|| parent.as_or_node().map(|n| n.left()));
    if let Some(left) = left {
        return left.location().start_offset() == start;
    }
    // `(if _ <any_def _>)` / `({while until} _ any_def)`: the def alone in the branch or body.
    if parent.as_statements_node().is_none_or(|s| s.body().iter().count() != 1) || ancestors.len() < 2 {
        return false;
    }
    match &ancestors[ancestors.len() - 2] {
        g @ Node::IfNode { .. } => {
            let i = g.as_if_node().unwrap();
            i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if") && i.end_keyword_loc().is_none()
        }
        g @ Node::UnlessNode { .. } => g.as_unless_node().unwrap().end_keyword_loc().is_none(),
        g @ Node::WhileNode { .. } => g.as_while_node().unwrap().closing_loc().is_none(),
        g @ Node::UntilNode { .. } => g.as_until_node().unwrap().closing_loc().is_none(),
        _ => false,
    }
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
        // unparenthesized_arguments?: `def foo a` gets its parentheses when made endless.
        let unparenthesized = has_args && def.lparen_loc().is_none();
        let padding = if helpers::alignment::cop_enabled(cx, "Layout/SpaceInsideParens")
            && cx.config.for_cop("Layout/SpaceInsideParens").str("EnforcedStyle") != Some("no_space")
        {
            " "
        } else {
            ""
        };
        let endless_args = if unparenthesized { format!("({padding}{args}{padding})") } else { args.to_string() };
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

        let sig_end = if has_args { args_range.unwrap().1 } else { def.name_loc().end_offset() };
        let def_end = def.end_keyword_loc().map_or(end, |e| e.end_offset());
        let body_bounds = body.as_ref().map(|(b, _)| (b.location().start_offset(), b.location().end_offset()));
        // can_be_made_endless?: no comment in the text the rewrite drops, and a body that keeps
        // its meaning in an endless definition.
        let can_be_endless = body.as_ref().is_some_and(|(b, _)| {
            let (bs, be) = body_bounds.unwrap();
            let dropped = [(sig_end.min(bs), sig_end.max(bs)), (be.min(def_end), be.max(def_end))];
            let comment = src.comments.iter().any(|c| {
                (src.line_of(start)..=src.line_of(end)).contains(&src.line_of(c.start))
                    && dropped.iter().any(|&(a, z)| a <= c.start && c.end <= z)
            });
            !comment
                && helpers::endless_method_body_help::endless_method_body(
                    b,
                    helpers::endless_method_body_help::argument_receiving_call(cx.ancestors).as_ref(),
                    cx.config.target_ruby,
                    false,
                )
        });
        let replacement = format!("def {receiver}{name}{endless_args} = {body_src}");
        let replacement = replacement.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0');
        // too_long_when_made_endless?: the whole line the definition would become.
        let too_long = || {
            helpers::alignment::cop_enabled(cx, "Layout/LineLength") && {
                let max = helpers::statement_modifier::line_length_max(&cx.config.for_cop("Layout/LineLength"));
                let kw = def.def_keyword_loc().start_offset();
                let before = &src.text[src.line_start(src.line_of(kw))..kw];
                let after = def.end_keyword_loc().map_or("", |e| {
                    let rest = &src.text[e.end_offset()..];
                    &rest[..rest.find('\n').unwrap_or(rest.len())]
                });
                let line = format!("{before}{replacement}{after}");
                helpers::statement_modifier::line_length(cx, &line) as f64 > max
            }
        };
        let ambiguous = ambiguous_when_made_endless(node, cx.ancestors);
        let to_endless = |cx: &mut Cx, msg: &str| {
            let (bs, be) = body_bounds.unwrap();
            let name_end = def.name_loc().end_offset();
            let first_arg = def.parameters().map(|p| p.location().start_offset()).filter(|_| unparenthesized);
            cx.add_fix(start, end, msg, |c| {
                match first_arg {
                    Some(a) => {
                        c.replace(name_end, a, "(");
                        c.replace(sig_end.min(bs), sig_end.max(bs), ") = ");
                    }
                    None => c.replace(sig_end.min(bs), sig_end.max(bs), " = "),
                }
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
                } else if !endless && can_be_endless && !too_long() && !ambiguous && !replacement.contains('\n') {
                    to_endless(cx, MSG_REQUIRE_SINGLE);
                }
            }
            "require_always" if !endless && can_be_endless && !too_long() && !ambiguous => {
                to_endless(cx, MSG_REQUIRE_ALWAYS);
            }
            _ => {}
        }
    }
}

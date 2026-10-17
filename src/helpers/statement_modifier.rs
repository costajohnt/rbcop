//! RuboCop's StatementModifier mixin (with the LineLengthHelp pieces it uses), shared by
//! Style/IfUnlessModifier and Style/WhileUntilModifier.

use crate::directives::DIRECTIVE;
use crate::helpers::alignment::{configured_indentation_width, cop_enabled};
use crate::helpers::line_length_help::{find_excessive_range, last_uri, uri_regex};
use crate::helpers::node_types::is_assignment;
use crate::helpers::parser_parent::parser_parent;
use crate::prelude::*;

/// An if/unless/while/until node in rubocop-ast terms.
pub struct Stmt<'pr> {
    pub keyword: &'pr str,
    pub keyword_loc: (usize, usize),
    pub condition: Node<'pr>,
    /// The single body statement, or the StatementsNode (parser `begin`) for several.
    pub body: Option<Node<'pr>>,
    /// `loc.end` (None in modifier form).
    pub end_loc: Option<(usize, usize)>,
    pub range: (usize, usize),
}

impl<'pr> Stmt<'pr> {
    pub fn of(node: &Node<'pr>) -> Option<Self> {
        let l = node.location();
        let range = (l.start_offset(), l.end_offset());
        if let Some(c) = crate::helpers::if_node::cond(node) {
            let end_loc = if c.has_end {
                match node {
                    Node::IfNode { .. } => node.as_if_node().unwrap().end_keyword_loc(),
                    _ => node.as_unless_node().unwrap().end_keyword_loc(),
                }
                .map(|e| (e.start_offset(), e.end_offset()))
            } else {
                None
            };
            return Some(Stmt { keyword: c.keyword, keyword_loc: c.keyword_loc?, condition: c.condition, body: c.body, end_loc, range });
        }
        let (kw, cond, stmts, closing) = match node {
            Node::WhileNode { .. } => {
                let w = node.as_while_node().unwrap();
                (w.keyword_loc(), w.predicate(), w.statements(), w.closing_loc())
            }
            Node::UntilNode { .. } => {
                let w = node.as_until_node().unwrap();
                (w.keyword_loc(), w.predicate(), w.statements(), w.closing_loc())
            }
            _ => return None,
        };
        let body = stmts.and_then(|s| {
            let v: Vec<Node> = s.body().iter().collect();
            match v.len() {
                0 => None,
                1 => v.into_iter().next(),
                _ => Some(s.as_node()),
            }
        });
        Some(Stmt {
            keyword: text(&kw),
            keyword_loc: (kw.start_offset(), kw.end_offset()),
            condition: cond,
            body,
            end_loc: closing.map(|e| (e.start_offset(), e.end_offset())),
            range,
        })
    }

    pub fn modifier_form(&self) -> bool {
        self.end_loc.is_none()
    }
}

fn first_line(cx: &Cx, s: &Stmt) -> usize {
    cx.src.line_of(s.range.0)
}

/// StatementModifier#non_eligible_node? (the base version).
pub fn non_eligible_node(cx: &Cx, s: &Stmt) -> bool {
    if s.modifier_form() {
        return true;
    }
    let src = cx.src.slice(s.range.0, s.range.1);
    let nonempty = src.split_inclusive('\n').filter(|l| l.bytes().any(|b| !b" \t\r\n\x0c\x0b".contains(&b))).count();
    nonempty > 3
        || cx.src.comment_on_line(cx.src.line_of(s.range.1)).is_some()
        || (first_line_comment(cx, s).is_some() && code_after(cx, s).is_some())
}

/// StatementModifier#non_eligible_body? (the base version).
pub fn non_eligible_body(cx: &Cx, body: Option<&Node>) -> bool {
    let Some(body) = body else { return true };
    let l = body.location();
    l.start_offset() == l.end_offset()
        || matches!(body, Node::StatementsNode { .. } | Node::ParenthesesNode { .. })
        || {
            let (a, b) = (cx.src.line_of(l.start_offset()), cx.src.line_of(l.end_offset()));
            cx.src.comments.iter().any(|c| (a..=b).contains(&cx.src.line_of(c.start)))
        }
}

struct Lvasgn {
    found: bool,
}

impl<'pr> ruby_prism::Visit<'pr> for Lvasgn {
    fn visit_local_variable_write_node(&mut self, _: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.found = true;
    }
    fn visit_local_variable_target_node(&mut self, _: &ruby_prism::LocalVariableTargetNode<'pr>) {
        self.found = true;
    }
    fn visit_local_variable_operator_write_node(&mut self, _: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.found = true;
    }
    fn visit_local_variable_or_write_node(&mut self, _: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.found = true;
    }
    fn visit_local_variable_and_write_node(&mut self, _: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.found = true;
    }
    // Pattern variables are parser `match_var`, not `lvasgn`.
    fn visit_match_predicate_node(&mut self, node: &ruby_prism::MatchPredicateNode<'pr>) {
        self.visit(&node.value());
    }
    fn visit_match_required_node(&mut self, node: &ruby_prism::MatchRequiredNode<'pr>) {
        self.visit(&node.value());
    }
    // A named-capture match is parser's `match_with_lvasgn` (regexp and receiver only, #366).
    fn visit_match_write_node(&mut self, node: &ruby_prism::MatchWriteNode<'pr>) {
        self.visit_call_node(&node.call());
    }
    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        if let Some(s) = node.statements() {
            self.visit_statements_node(&s);
        }
    }
}

/// StatementModifier#non_eligible_condition?: an `lvasgn` anywhere in the condition.
pub fn non_eligible_condition(condition: &Node) -> bool {
    let mut v = Lvasgn { found: false };
    ruby_prism::Visit::visit(&mut v, condition);
    v.found
}

/// AutocorrectLogic#max_line_length.
pub fn max_line_length(cx: &Cx) -> Option<usize> {
    if !cop_enabled(cx, "Layout/LineLength") {
        return None;
    }
    Some(cx.config.for_cop("Layout/LineLength").int("Max").unwrap_or(120).max(0) as usize)
}

fn tab_indentation_width(cx: &Cx) -> usize {
    cx.config
        .for_cop("Layout/IndentationStyle")
        .int("IndentationWidth")
        .unwrap_or_else(|| configured_indentation_width(cx))
        .max(0) as usize
}

/// LineLengthHelp#indentation_difference.
fn indentation_difference(cx: &Cx, line: &str) -> usize {
    // `line.match?(/^[^\t]/)` checks every line start of the string.
    if line.split('\n').any(|l| l.chars().next().is_some_and(|c| c != '\t')) {
        return 0;
    }
    let index = line.char_indices().find(|&(_, c)| c != '\t').map_or(0, |(i, _)| i);
    index * tab_indentation_width(cx).saturating_sub(1)
}

/// LineLengthHelp#line_length.
pub fn line_length(cx: &Cx, line: &str) -> usize {
    line.chars().count() + indentation_difference(cx, line)
}

/// StatementModifier#modifier_fits_on_single_line?
pub fn modifier_fits_on_single_line(cx: &Cx, s: &Stmt, parenthesize: bool) -> bool {
    let Some(max) = max_line_length(cx) else { return true };
    line_length(cx, &line_in_modifier_form(cx, s, parenthesize)) <= max
}

fn line_in_modifier_form(cx: &Cx, s: &Stmt, parenthesize: bool) -> String {
    let ls = cx.src.line_start(cx.src.line_of(s.keyword_loc.0));
    let code_before = cx.src.slice(ls, s.keyword_loc.0);
    format!("{code_before}{}{}", to_modifier_form(cx, s, parenthesize), code_after(cx, s).unwrap_or(""))
}

/// StatementModifier#to_modifier_form.
pub fn to_modifier_form(cx: &Cx, s: &Stmt, parenthesize: bool) -> String {
    let mut parts = Vec::new();
    if let Some(b) = &s.body {
        parts.push(if_body_source(b));
    }
    parts.push(s.keyword.to_string());
    parts.push(node_text(&s.condition).to_string());
    let expression = parts.join(" ");
    let parenthesized = if parenthesize { format!("({expression})") } else { expression };
    match first_line_comment(cx, s) {
        Some(c) => format!("{parenthesized} {c}"),
        None => parenthesized,
    }
}

fn if_body_source(body: &Node) -> String {
    if let Some(c) = body.as_call_node()
        && c.block().is_none_or(|b| b.as_block_node().is_none())
        && method_name(&c) != "[]="
        && omitted_value_in_last_hash_arg(&c)
    {
        let start = body.location().start_offset();
        let end = match (c.message_loc(), c.call_operator_loc()) {
            // implicit_call? (`a.()`): up to the dot
            (None, Some(dot)) => dot.end_offset(),
            (Some(sel), _) => sel.end_offset(),
            (None, None) => return node_text(body).to_string(),
        };
        let method_source = std::str::from_utf8(&body.location().as_slice()[..end - start]).unwrap_or("");
        let args: Vec<&str> = send_args(&c).iter().map(node_text).collect();
        return format!("{method_source}({})", args.join(", "));
    }
    node_text(body).to_string()
}

fn omitted_value_in_last_hash_arg(c: &ruby_prism::CallNode) -> bool {
    let args = send_args(c);
    let Some(last) = args.last() else { return false };
    let elements = match last {
        Node::HashNode { .. } => last.as_hash_node().unwrap().elements(),
        Node::KeywordHashNode { .. } => last.as_keyword_hash_node().unwrap().elements(),
        _ => return false,
    };
    elements
        .iter()
        .filter_map(|e| e.as_assoc_node())
        .last()
        .is_some_and(|p| p.value().as_implicit_node().is_some())
}

/// StatementModifier#first_line_comment: the comment on the node's first line, unless it
/// disables this cop.
pub fn first_line_comment<'a>(cx: &Cx<'a, '_>, s: &Stmt) -> Option<&'a str> {
    let c = cx.src.comment_on_line(first_line(cx, s))?;
    let text = cx.src.slice(c.start, c.end);
    // Ruby's \s is ASCII-only.
    let re = Regex::new(&format!(
        r"#[\t\n\x0B\x0C\r ]*rubocop[\t\n\x0B\x0C\r ]*:[\t\n\x0B\x0C\r ]*(disable|todo)[\t\n\x0B\x0C\r ]*([^,],)*[\t\n\x0B\x0C\r ]*(all|{})",
        regex::escape(cx.cop)
    ))
    .unwrap();
    (!re.is_match(text)).then_some(text)
}

/// StatementModifier#code_after: the rest of the `end` line, if any.
pub fn code_after<'a>(cx: &Cx<'a, '_>, s: &Stmt) -> Option<&'a str> {
    let (_, end) = s.end_loc?;
    let line_end = cx.src.text[end..].find('\n').map_or(cx.src.text.len(), |i| end + i);
    let code = cx.src.slice(end, line_end);
    (!code.is_empty()).then_some(code)
}

/// StatementModifier#parenthesize? for the visited node (its Prism ancestors).
pub fn parenthesize(ancestors: &[Node]) -> bool {
    let Some(parent) = parser_parent(ancestors) else { return false };
    if is_assignment(parent) || matches!(parent, Node::AndNode { .. } | Node::OrNode { .. }) {
        return true;
    }
    if matches!(parent, Node::ArrayNode { .. } | Node::AssocNode { .. }) {
        return true;
    }
    parent.as_call_node().is_some_and(|c| !is_csend(&c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// LineLengthHelp#find_excessive_range for URIs, in character columns.
fn uri_range(cx: &Cx, line: &str, max: usize) -> Option<(usize, usize)> {
    let re = uri_regex(&cx.config.for_cop("Layout/LineLength").strings("URISchemes"))?;
    find_excessive_range(line, last_uri(&re, line), max, indentation_difference(cx, line))
}

/// StatementModifier#acceptable_line_length? for an existing source line (1-based `line_number`).
pub fn acceptable_line_length(cx: &Cx, line: &str, line_number: usize) -> bool {
    let Some(max) = max_line_length(cx) else { return true };
    let len = line_length(cx, line);
    if len <= max {
        return true;
    }
    if crate::directives::is_disabled(&cx.facts.disabled, "Layout/LineLength", line_number, line_number) {
        return true;
    }
    let ll = cx.config.for_cop("Layout/LineLength");
    // `ll['AllowedPatterns'] || ll['IgnoredPatterns'] || []`, then `.map`: a String raises (a
    // cop error).
    let truthy = |k: &str| ll.get(k).filter(|v| !matches!(v, Value::Null | Value::Bool(false)));
    let patterns: Vec<crate::config::value::RubyRegex> = match truthy("AllowedPatterns").or_else(|| truthy("IgnoredPatterns")) {
        None => Vec::new(),
        Some(Value::Seq(items)) => items
            .iter()
            .filter_map(|v| match v {
                Value::Regex(r) => crate::config::value::ruby_regex(r),
                v => v.as_string().and_then(|s| crate::config::value::ruby_regex(&s)),
            })
            .collect(),
        Some(_) => panic!("undefined method 'map' for Layout/LineLength AllowedPatterns"),
    };
    if patterns.iter().any(|r| r.is_match(line)) {
        return true;
    }
    let allow_directives = match ll.bool("IgnoreCopDirectives") {
        Some(v) => v,
        None => ll.bool("AllowCopDirectives").unwrap_or(false),
    };
    if allow_directives
        && let Some(c) = cx.src.comment_on_line(line_number)
        && crate::directives::parse(cx.src.slice(c.start, c.end)).is_some()
    {
        let before = DIRECTIVE.split(line).next().unwrap_or("");
        return before.trim_end_matches([' ', '\t', '\n', '\r', '\x0c', '\x0b', '\0']).chars().count() <= max;
    }
    if ll.bool("AllowURI") != Some(true) {
        return false;
    }
    uri_range(cx, line, max).is_some_and(|(b, e)| b < max && e == len)
}

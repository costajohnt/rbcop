use crate::directives::DIRECTIVE;
use crate::prelude::*;
use helpers::line_length_help::{find_excessive_range, last_match, last_uri, uri_regex};

pub const NAME: &str = "Layout/LineLength";
pub static COP: LineLength = LineLength;
pub struct LineLength;

static QUALIFIED_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:[A-Z][A-Za-z0-9_]*::)+[A-Za-z_][A-Za-z0-9_]*\b").unwrap());
static RBS_ANNOTATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:#:|#\[.+\]|#\|)").unwrap());

struct Settings {
    max: usize,
    tab_width: usize,
}

impl Settings {
    fn indentation_difference(&self, line: &str) -> usize {
        if self.tab_width == 0 {
            return 0;
        }
        line.bytes().take_while(|&b| b == b'\t').count() * (self.tab_width - 1)
    }

    fn line_length(&self, line: &str) -> usize {
        line.chars().count() + self.indentation_difference(line)
    }

    /// LineLengthHelp#find_excessive_range, in character columns.
    fn excessive_range(&self, line: &str, last: Option<(usize, usize)>) -> Option<(usize, usize)> {
        find_excessive_range(line, last, self.max, self.indentation_difference(line))
    }
}

/// Per-file state: where each line could be broken (CheckLineBreakable), and endless methods.
#[derive(Default)]
struct Breakable {
    init: bool,
    /// 0-based line index to the break range (the line break is inserted before it).
    by_line: std::collections::HashMap<usize, (usize, usize)>,
    /// 0-based line index of an endless method definition to its multiline rewrite (range, replacement), if correctable.
    endless: std::collections::HashMap<usize, Option<(usize, usize, String)>>,
}

fn max_of(cx: &Cx) -> usize {
    cx.cfg.int("Max").unwrap_or(120).max(0) as usize
}

/// LineLength#on_new_investigation: break points after semicolons, earliest per line wins.
fn init(cx: &mut Cx) {
    if cx.state::<Breakable>().init {
        return;
    }
    let src = cx.src;
    let mut found = Vec::new();
    if src.text.contains(';') {
        for t in src.tokens().iter().rev().filter(|t| t.kind == sys::PM_TOKEN_SEMICOLON) {
            // breakable_range_after_semicolon: the next character, if it is on the same line and
            // neither a line break nor another semicolon.
            match src.text[t.end..].chars().next() {
                Some(c) if c != '\n' && c != '\r' && c != ';' => found.push((src.line_of(t.end) - 1, (t.end, t.end + c.len_utf8()))),
                _ => {}
            }
        }
    }
    let st = cx.state::<Breakable>();
    st.init = true;
    for (line, pos) in found {
        st.by_line.insert(line, pos);
    }
}

struct Element {
    start: usize,
    end: usize,
    heredoc: bool,
}

fn element(n: &Node) -> Element {
    let loc = n.location();
    Element { start: loc.start_offset(), end: loc.end_offset(), heredoc: is_heredoc(n) }
}

/// CheckLineBreakable#process_args: an unbraced trailing hash contributes its pairs.
fn call_elements(call: &ruby_prism::CallNode) -> Vec<Element> {
    let args = send_args(call);
    let mut out: Vec<Element> = args.iter().map(element).collect();
    if let Some(last) = args.last().filter(|n| kind::of(n) == kind::KEYWORD_HASH_NODE) {
        out.pop();
        out.extend(last.as_keyword_hash_node().unwrap().elements().iter().map(|e| element(&e)));
    }
    out
}

/// Elements of a parser-shaped ancestor if it is a hash/array/call: Some((elements, starts_with_bracket)).
fn collection_elements(n: &Node) -> Option<(Vec<Element>, bool)> {
    match kind::of(n) {
        kind::ARRAY_NODE => Some((n.as_array_node().unwrap().elements().iter().map(|e| element(&e)).collect(), true)),
        kind::HASH_NODE => Some((n.as_hash_node().unwrap().elements().iter().map(|e| element(&e)).collect(), true)),
        kind::KEYWORD_HASH_NODE => {
            Some((n.as_keyword_hash_node().unwrap().elements().iter().map(|e| element(&e)).collect(), false))
        }
        kind::CALL_NODE => Some((call_elements(&n.as_call_node().unwrap()), true)),
        _ => None,
    }
}

fn breakable_collection(elements: &[Element], starts_with_bracket: bool) -> bool {
    starts_with_bracket && elements.len() >= 2
}

/// CheckLineBreakable#children_could_be_broken_up?
fn children_could_be_broken_up(cx: &Cx, children: &[Element]) -> bool {
    let line = |o: usize| cx.src.line_of(o);
    if children.is_empty() || line(children[0].start) == line(children.last().unwrap().end) {
        return false;
    }
    let mut last_seen = 0usize;
    for (i, c) in children.iter().enumerate() {
        if i > 0 && last_seen >= line(c.start) {
            return true;
        }
        last_seen = line(c.end);
    }
    false
}

/// CheckLineBreakable#extract_breakable_node_from_elements; `range` is the node's parser range.
fn breakable_element(
    cx: &Cx,
    range: (usize, usize),
    elements: &[Element],
    call: Option<(bool, bool)>,
    def_last_line: Option<usize>,
    is_array: bool,
) -> Option<(usize, usize)> {
    let max = max_of(cx);
    let src = cx.src;
    let first_line = src.line_of(range.0);
    if src.lines.get(first_line - 1).is_none_or(|l| l.chars().count() <= max) {
        return None;
    }
    if src.comments.iter().any(|c| src.line_of(c.start) == first_line) {
        return None;
    }
    // safe_to_ignore?
    let multiline = match def_last_line {
        Some(last) => first_line != last,
        None => first_line != src.line_of(range.1.saturating_sub(1).max(range.0)),
    };
    if multiline {
        return None;
    }
    let anc = parser_ancestor_ranges(cx.ancestors);
    for &(i, start, _) in &anc {
        if src.line_of(start) != first_line {
            break;
        }
        if let Some((els, bracket)) = collection_elements(&cx.ancestors[i])
            && breakable_collection(&els, bracket)
        {
            return None;
        }
    }
    for &(i, _, _) in &anc {
        if let Some((els, bracket)) = collection_elements(&cx.ancestors[i])
            && breakable_collection(&els, bracket)
        {
            if children_could_be_broken_up(cx, &els) {
                return None;
            }
            break;
        }
    }
    // extract_first_element_over_column_limit
    let mut elements: Vec<&Element> = elements.iter().collect();
    if let Some((parenthesized, first_is_heredoc)) = call
        && !parenthesized
        && !first_is_heredoc
    {
        elements.remove(0);
    }
    let within = |e: &&Element| src.column(e.start) <= max && src.line_of(e.start) == first_line;
    let mut i = 0;
    while elements.get(i).is_some_and(within) {
        i += 1;
    }
    // shift_elements_for_heredoc_arg: only calls and arrays.
    if (call.is_some() || is_array)
        && let Some(h) = elements.iter().position(|e| e.heredoc) {
            if h == 0 {
                return None;
            }
            if h < i {
                i = h + 1;
            }
        }
    let chosen = if i == 0 { elements.first()? } else { elements[i - 1] };
    Some((chosen.start, chosen.end))
}

/// LineLength endless-method corrections, as (range, replacement): EndlessMethodRewriter#correct_to_multiline,
/// or under Style/EndlessMethod `require_always` correct_endless_method_block_to_multiline, which
/// only applies to a single-line braces block body.
fn endless_rewrite(cx: &Cx, def: &ruby_prism::DefNode) -> Option<(usize, usize, String)> {
    let style = cx.config.for_cop("Style/EndlessMethod");
    let require_always =
        style.truthy("Enabled") && style.str("EnforcedStyle") == Some("require_always");
    let src = cx.src;
    let loc = def.location();
    let column = src.column(loc.start_offset());
    let indent = |offset: usize| " ".repeat(column + offset);
    let body = def.body()?;
    if !require_always {
        let receiver = match (def.receiver(), def.operator_loc()) {
            (Some(r), Some(op)) => format!("{}{}", node_text(&r), text(&op)),
            _ => String::new(),
        };
        let args = match (def.parameters(), def.lparen_loc(), def.rparen_loc()) {
            (Some(_), Some(l), Some(r)) => src.slice(l.start_offset(), r.end_offset()),
            _ => "",
        };
        // configured_indentation_width
        let width = cx.cfg.int("IndentationWidth").or_else(|| cx.config.for_cop("Layout/IndentationWidth").int("Width"));
        let width = width.unwrap_or(2).max(0) as usize;
        let name = String::from_utf8_lossy(def.name().as_slice());
        let replacement =
            format!("def {receiver}{name}{args}\n{}{}\n{}end", indent(width), node_text(&body), indent(0));
        return Some((loc.start_offset(), loc.end_offset(), replacement));
    }
    // correctable_endless_method_block?: body is a single-line `{ }` block with a body.
    let body = body.as_statements_node().and_then(|s| s.body().iter().next()).unwrap_or(body);
    let call = body.as_call_node()?;
    let block = call.block().and_then(|b| b.as_block_node())?;
    let cloc = call.location();
    let ok = block.opening_loc().as_slice() == b"{"
        && src.line_of(cloc.start_offset()) == src.line_of(cloc.end_offset().saturating_sub(1))
        && block.body().is_some()
        && !call.receiver().is_some_and(|r| contains_heredoc(&r));
    if !ok {
        return None;
    }
    let (send_start, send_end) = send_range(&call);
    let args = block
        .parameters()
        .and_then(|p| p.as_block_parameters_node())
        .filter(|p| p.parameters().is_some() || p.locals().iter().next().is_some())
        .map(|p| format!(" {}", node_text(&p.as_node())))
        .unwrap_or_default();
    let replacement = format!(
        "{} do{args}\n{}{}\n{}end",
        src.slice(send_start, send_end),
        indent(2),
        node_text(&block.body()?),
        indent(0)
    );
    Some((cloc.start_offset(), cloc.end_offset(), replacement))
}

fn record_node_break(cx: &mut Cx, pos: (usize, usize)) {
    let line = cx.src.line_of(pos.0) - 1;
    cx.state::<Breakable>().by_line.entry(line).or_insert(pos);
}

/// LineLength#check_for_breakable_block (on_block): overwrites any earlier break point on the line.
fn check_block(cx: &mut Cx, start: usize, block: &Node, receiver: Option<Node>, lambda: bool) {
    let src = cx.src;
    let (opening, closing, params) = match kind::of(block) {
        kind::BLOCK_NODE => {
            let b = block.as_block_node().unwrap();
            (b.opening_loc(), b.closing_loc(), b.parameters())
        }
        _ => {
            let l = block.as_lambda_node().unwrap();
            (l.opening_loc(), l.closing_loc(), l.parameters())
        }
    };
    // BlockNode#single_line? compares the `{`/`do` line with the `}`/`end` line; the break is
    // recorded on the line where the block expression (receiver included) starts.
    if src.line_of(opening.start_offset()) != src.line_of(closing.start_offset()) {
        return;
    }
    if receiver.as_ref().is_some_and(|r| contains_heredoc(r)) {
        return;
    }
    let arguments_end = params.as_ref().and_then(|p| p.as_block_parameters_node()).and_then(|p| {
        let has_args = p.parameters().is_some() || p.locals().iter().next().is_some();
        if has_args { p.closing_loc().map(|c| c.start_offset()) } else { None }
    });
    let range_begin = match arguments_end {
        Some(pipe) if !lambda => pipe,
        _ if opening.as_slice() == b"{" => opening.start_offset(),
        _ => opening.start_offset() + 1,
    };
    let pos = range_begin + 1;
    let line = src.line_of(start) - 1;
    cx.state::<Breakable>().by_line.insert(line, (pos, pos + src.text[pos..].chars().next().map_or(0, char::len_utf8)));
}

fn contains_heredoc(n: &Node) -> bool {
    struct Find(bool);
    impl<'pr> ruby_prism::Visit<'pr> for Find {
        fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
            self.0 |= node.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"<<"));
        }
        fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
            self.0 |= node.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"<<"));
            ruby_prism::visit_interpolated_string_node(self, node);
        }
        fn visit_x_string_node(&mut self, node: &ruby_prism::XStringNode<'pr>) {
            self.0 |= node.opening_loc().as_slice().starts_with(b"<<");
        }
        fn visit_interpolated_x_string_node(&mut self, node: &ruby_prism::InterpolatedXStringNode<'pr>) {
            self.0 |= node.opening_loc().as_slice().starts_with(b"<<");
            ruby_prism::visit_interpolated_x_string_node(self, node);
        }
    }
    let mut f = Find(false);
    ruby_prism::Visit::visit(&mut f, n);
    f.0
}

/// Parameters of a def in source order (parser `args` children).
fn def_params(def: &ruby_prism::DefNode) -> Vec<Element> {
    let Some(p) = def.parameters() else { return Vec::new() };
    let mut nodes: Vec<Node> = Vec::new();
    nodes.extend(p.requireds().iter());
    nodes.extend(p.optionals().iter());
    nodes.extend(crate::ast::parser_rest(p.rest()));
    nodes.extend(p.posts().iter());
    nodes.extend(p.keywords().iter());
    nodes.extend(p.keyword_rest());
    if let Some(b) = p.block() {
        nodes.push(b.as_node());
    }
    let mut els: Vec<Element> = nodes.iter().map(element).collect();
    els.sort_by_key(|e| e.start);
    els
}

impl Cop for LineLength {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::SUPER_NODE,
            kind::FORWARDING_SUPER_NODE,
            kind::DEF_NODE,
            kind::ARRAY_NODE,
            kind::HASH_NODE,
            kind::LAMBDA_NODE,
            kind::RESCUE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        init(cx);
        match kind::of(node) {
            kind::CALL_NODE => {
                let call = node.as_call_node().unwrap();
                // on_block runs before on_send for the block's own call.
                if let Some(block) = call.block().filter(|b| kind::of(b) == kind::BLOCK_NODE) {
                    let loc = node.location();
                    let lambda = method_name(&call) == "lambda";
                    check_block(cx, loc.start_offset(), &block, call.receiver(), lambda);
                }
                let mut recv = call.receiver();
                while let Some(r) = recv {
                    if is_heredoc(&r) {
                        return;
                    }
                    recv = r.as_call_node().and_then(|c| c.receiver());
                }
                let elements = call_elements(&call);
                if !breakable_collection(&elements, true) {
                    return;
                }
                let first_is_heredoc = call_args(&call).first().is_some_and(is_heredoc);
                let range = send_range(&call);
                // `parenthesized?`: parser gives `a[x, y]` no `loc.begin`.
                let parenthesized = call.opening_loc().is_some_and(|l| l.as_slice() == b"(");
                if let Some(pos) = breakable_element(cx, range, &elements, Some((parenthesized, first_is_heredoc)), None, false) {
                    record_node_break(cx, pos);
                }
            }
            // on_block for the block of `super(...)`/zsuper (no receiver, not a lambda); RuboCop's
            // on_send does not run for `super`.
            kind::SUPER_NODE | kind::FORWARDING_SUPER_NODE => {
                if let Some(block) = helpers::node_types::literal_block(node) {
                    check_block(cx, node.location().start_offset(), &block.as_node(), None, false);
                }
            }
            kind::RESCUE_NODE => {
                // Parser wraps `rescue A, B` in an unbracketed `array`, which on_array checks.
                // ponytail: that array is not a parser ancestor of calls inside the list.
                let elements: Vec<Element> = helpers::resbody::exceptions(node).iter().map(element).collect();
                if !breakable_collection(&elements, true) {
                    return;
                }
                let range = (elements[0].start, elements.last().unwrap().end);
                if let Some(pos) = breakable_element(cx, range, &elements, None, None, true) {
                    record_node_break(cx, pos);
                }
            }
            kind::LAMBDA_NODE => {
                let loc = node.location();
                check_block(cx, loc.start_offset(), node, None, true);
            }
            kind::DEF_NODE => {
                let def = node.as_def_node().unwrap();
                if def.equal_loc().is_some() && def.end_keyword_loc().is_none() {
                    // Endless method: tracked per line, last one wins.
                    let line = cx.src.line_of(node.location().start_offset()) - 1;
                    let rewrite = endless_rewrite(cx, &def);
                    cx.state::<Breakable>().endless.insert(line, rewrite);
                    return;
                }
                let params = def_params(&def);
                if params.len() < 2 {
                    return;
                }
                let last_line = cx.src.line_of(params.last().unwrap().end.saturating_sub(1));
                let loc = node.location();
                if let Some(pos) = breakable_element(cx, (loc.start_offset(), loc.end_offset()), &params, None, Some(last_line), false) {
                    record_node_break(cx, pos);
                }
            }
            _ => {
                let (elements, bracket) = collection_elements(node).unwrap();
                if !breakable_collection(&elements, bracket) {
                    return;
                }
                let loc = node.location();
                if let Some(pos) = breakable_element(cx, (loc.start_offset(), loc.end_offset()), &elements, None, None, kind::of(node) == kind::ARRAY_NODE) {
                    record_node_break(cx, pos);
                }
            }
        }
    }

    fn on_file(&self, cx: &mut Cx) {
        init(cx);
        let breaks = std::mem::take(&mut cx.state::<Breakable>().by_line);
        let endless = std::mem::take(&mut cx.state::<Breakable>().endless);
        let cfg = cx.cfg;
        let max = max_of(cx);
        let indentation_width = cx.config.for_cop("Layout/IndentationStyle").int("IndentationWidth");
        let width = indentation_width.or_else(|| cx.config.for_cop("Layout/IndentationWidth").int("Width")).unwrap_or(2);
        let s = Settings { max, tab_width: width.max(0) as usize };
        let allow_uri = cfg.bool("AllowURI").unwrap_or(true);
        let allow_qn = cfg.bool("AllowQualifiedName").unwrap_or(true);
        let allow_directives = match cfg.bool("IgnoreCopDirectives") {
            Some(v) => v,
            None => cfg.bool("AllowCopDirectives").unwrap_or(true),
        };
        let allow_rbs = cfg.bool("AllowRBSInlineAnnotation").unwrap_or(false);
        let mut allowed = cfg.regexes("AllowedPatterns");
        allowed.extend(cfg.regexes("IgnoredPatterns"));
        let heredoc_ok = |delim: &str| match cfg.get("AllowHeredoc") {
            Some(Value::Bool(b)) => *b,
            Some(Value::Seq(list)) => list.iter().any(|v| v.as_string().as_deref() == Some(delim)),
            _ => false,
        };
        let uri = if allow_uri { uri_regex(&cfg.strings("URISchemes")) } else { None };
        let src = cx.src;

        for (i, line) in src.lines.iter().enumerate() {
            let lineno = i + 1;
            let len = s.line_length(line);
            if len <= max {
                continue;
            }
            if allowed.iter().any(|r| r.is_match(line))
                || (i == 0 && line.starts_with("#!"))
                || cx.facts.heredocs.iter().any(|h| h.first <= lineno && lineno < h.terminator && heredoc_ok(&h.delimiter))
            {
                continue;
            }
            let start = src.line_start(lineno);
            // source_range(buffer, line, col...): columns past the line's end (line_length counts
            // tab indentation as several columns) run on into the following lines.
            let at = |col: usize| start + src.text[start..].char_indices().nth(col).map_or(src.text.len() - start, |(b, _)| b);
            // register_offense: insert a line break before the breakable range, if any.
            let break_line = |c: &mut Corrector| {
                if let Some(&(b, e)) = breaks.get(&i) {
                    c.insert_before(b, e, "\n");
                }
            };
            if let Some(rewrite) = endless.get(&i) {
                let from = max.saturating_sub(s.indentation_difference(line));
                cx.add_fix(at(from), at(len), format!("Line is too long. [{len}/{max}]"), |c| {
                    if let Some((b, e, r)) = rewrite {
                        c.replace(*b, *e, r);
                    }
                });
                continue;
            }
            let comment = src.comment_on_line(lineno).map(|c| src.slice(c.start, c.end));
            if allow_rbs
                && comment.is_some_and(|c| RBS_ANNOTATION.is_match(c))
            {
                continue;
            }
            if allow_directives && comment.is_some_and(|c| crate::directives::parse(c).is_some()) {
                let before = DIRECTIVE.split(line).next().unwrap_or("").trim_end().chars().count();
                if before > max {
                    cx.add_fix(at(max), at(before), format!("Line is too long. [{before}/{max}]"), break_line);
                }
                continue;
            }
            let mut range = None;
            if allow_uri || allow_qn {
                let u = uri.as_ref().and_then(|r| s.excessive_range(line, last_uri(r, line)));
                let q = if allow_qn { s.excessive_range(line, last_match(&QUALIFIED_NAME, line)) } else { None };
                let ok = |r: (usize, usize)| r.0 < max && r.1 == len;
                let allowed_combination = match (u, q) {
                    (Some(u), Some(q)) => ok(u) && ok(q),
                    (Some(r), None) | (None, Some(r)) => ok(r),
                    (None, None) => false,
                };
                if allowed_combination {
                    continue;
                }
                range = u.or(q);
            }
            let from = match range {
                Some((b, e)) if b < max => e,
                _ => max.saturating_sub(s.indentation_difference(line)),
            };
            // ponytail: SplitStrings break points (strings split with a backslash) are not modelled yet.
            cx.add_fix(at(from), at(len), format!("Line is too long. [{len}/{max}]"), break_line);
        }
    }
}

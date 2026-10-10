use crate::prelude::*;
use helpers::check_assignment::{KINDS, extract_rhs, first_part_of_call_chain};
use helpers::end_keyword_alignment::{begins_its_line, effective_column};
use ruby_prism::{BeginNode, Location};
use std::collections::HashSet;

pub const NAME: &str = "Layout/IndentationWidth";
pub static COP: IndentationWidth = IndentationWidth;
pub struct IndentationWidth;

#[derive(Default)]
struct State {
    /// `ignore_node`d defs / conditionals, by span.
    ignored: HashSet<(usize, usize)>,
    /// AllowedPatterns (+ IgnoredPatterns), compiled once per file.
    patterns: Option<Vec<Regex>>,
    /// `@offense_ranges` (`other_offense_in_same_range?`, only consulted when autocorrecting).
    offense_ranges: Vec<(usize, usize)>,
}

fn span(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

fn lstart(l: Location) -> usize {
    l.start_offset()
}

/// A parser-gem body node.
enum Body<'a, 'pr> {
    Child(&'a Node<'pr>),
    Single(Node<'pr>),
    /// `begin` (several statements)
    Begin(Vec<Node<'pr>>),
    /// `rescue` / `ensure` (implicit or explicit BeginNode): first body statement and range.
    Wrapped { node: BeginNode<'pr>, first: Option<Node<'pr>>, range: (usize, usize) },
}

impl<'a, 'pr> Body<'a, 'pr> {
    fn of(body: Option<Node<'pr>>) -> Option<Self> {
        let body = body?;
        if let Some(s) = body.as_statements_node() {
            let mut v: Vec<Node> = s.body().iter().collect();
            return match v.len() {
                0 => None,
                1 => Some(Body::Single(v.pop().unwrap())),
                _ => Some(Body::Begin(v)),
            };
        }
        if let Some(b) = body.as_begin_node() {
            if b.rescue_clause().is_some() || b.ensure_clause().is_some() {
                return Some(Self::wrapped(b));
            }
            if b.begin_keyword_loc().is_none() {
                return Self::of(b.statements().map(|s| s.as_node()));
            }
        }
        Some(Body::Single(body))
    }

    fn wrapped(b: BeginNode<'pr>) -> Self {
        let first = b.statements().and_then(|s| s.body().iter().next());
        let start = [
            b.statements().map(|s| s.location().start_offset()),
            b.rescue_clause().map(|r| r.location().start_offset()),
            b.else_clause().map(|e| e.location().start_offset()),
            b.ensure_clause().map(|e| e.location().start_offset()),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(b.location().start_offset());
        let end = if let Some(e) = b.ensure_clause() {
            e.statements().map_or(e.ensure_keyword_loc().end_offset(), |s| s.location().end_offset())
        } else if let Some(e) = b.else_clause() {
            e.statements().map_or(e.else_keyword_loc().end_offset(), |s| s.location().end_offset())
        } else if let Some(mut r) = b.rescue_clause() {
            while let Some(next) = r.subsequent() {
                r = next;
            }
            r.location().end_offset()
        } else {
            b.statements().map_or(start, |s| s.location().end_offset())
        };
        Body::Wrapped { node: b, first, range: (start, end) }
    }

    /// Start of the parser node (`body_node.loc`).
    fn start(&self) -> usize {
        match self {
            Body::Single(n) => span(n).0,
            Body::Child(n) => span(n).0,
            Body::Begin(v) => span(&v[0]).0,
            Body::Wrapped { first, range, .. } => first.as_ref().map_or(range.0, |f| span(f).0),
        }
    }
}

/// `bare_access_modifier?` (given whether the node is in macro scope).
fn bare_access_modifier(node: &Node, macro_scope: bool) -> bool {
    access_modifier(node, macro_scope, true)
}

/// `access_modifier?` (bare or with arguments).
fn access_modifier(node: &Node, macro_scope: bool, bare_only: bool) -> bool {
    let Some(c) = node.as_call_node() else { return false };
    if !macro_scope || c.receiver().is_some() || c.block().is_some_and(|b| b.as_block_node().is_some()) {
        return false;
    }
    let has_args = !send_args(&c).is_empty();
    if bare_only && has_args {
        return false;
    }
    matches!(c.name().as_slice(), b"public" | b"protected" | b"private" | b"module_function")
}

fn using_tabs(cx: &Cx) -> bool {
    cx.config.for_cop("Layout/IndentationStyle").str("EnforcedStyle") == Some("tabs")
}

fn width(cx: &Cx) -> i64 {
    helpers::alignment::configured_indentation_width(cx)
}

/// `line_indentation(range)`: the line's text before the range's column.
fn line_indentation<'a>(src: &crate::source::Source<'a>, off: usize) -> &'a str {
    &src.text[src.line_start(src.line_of(off))..off]
}

/// `column_offset_between(base, range)`, with the tab-aware override.
fn column_offset(cx: &Cx, base: usize, range: usize) -> i64 {
    let src = cx.src;
    if using_tabs(cx) && (line_indentation(src, base).contains('\t') || line_indentation(src, range).contains('\t')) {
        let visual = |off: usize| {
            let ind = line_indentation(src, off);
            ind.matches('\t').count() as i64 * width(cx) + ind.matches(' ').count() as i64
        };
        return visual(base) - visual(range);
    }
    effective_column(src, base) - effective_column(src, range)
}

fn first_non_space_char(line: &str) -> Option<usize> {
    line.chars().position(|c| !matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
}

/// What `check_indentation` needs to know about where the checked body sits.
struct Ctx {
    /// The body's begin children are in macro scope (for `starts_with_access_modifier?`).
    macro_scope: bool,
}

fn allowed_line(cx: &mut Cx, base: usize) -> bool {
    let src = cx.src;
    let line = src.lines.get(src.line_of(base) - 1).copied().unwrap_or("");
    let cfg = cx.cfg;
    let st = cx.state::<State>();
    let pats = st.patterns.get_or_insert_with(|| {
        cfg.regexes("AllowedPatterns").into_iter().chain(cfg.regexes("IgnoredPatterns")).collect()
    });
    pats.iter().any(|r| r.is_match(line))
}

/// `check_indentation(base_loc, body_node, style)`.
fn check_indentation(cx: &mut Cx, base: usize, body: Option<&Body>, style: &str, ctx: &Ctx) {
    if allowed_line(cx, base) {
        return;
    }
    let Some(body) = body else { return };
    let src = cx.src;
    let start = body.start();
    if src.line_of(start) == src.line_of(base) {
        return;
    }
    if let Body::Begin(v) = body
        && bare_access_modifier(&v[0], ctx.macro_scope) {
            return;
        }
    let line = src.lines.get(src.line_of(start) - 1).copied().unwrap_or("");
    if first_non_space_char(line) != Some(src.column(start)) {
        return;
    }
    if let Body::Wrapped { first: None, .. } = body {
        return;
    }
    let indentation = column_offset(cx, start, base);
    let width = width(cx);
    let delta = width - indentation;
    if delta == 0 {
        return;
    }
    // The offense node: a begin's first child.
    let wrapped;
    let (node, range): (&Node, (usize, usize)) = match body {
        Body::Single(n) => (n, span(n)),
        Body::Child(n) => (*n, span(n)),
        Body::Begin(v) => (&v[0], span(&v[0])),
        Body::Wrapped { node, range, .. } => {
            wrapped = node.as_node();
            (&wrapped, *range)
        }
    };
    let tabs = using_tabs(cx);
    let begin = range.0 as i64;
    let ind = if tabs { begin - line_indentation(src, range.0).chars().count() as i64 } else { begin - indentation };
    let (s, e) = if ind <= begin { (ind, begin) } else { (begin, ind) };
    let name = if style == "normal" { String::new() } else { format!(" {style}") };
    let msg = if tabs {
        let actual = if width == 0 { 0 } else { indentation.div_euclid(width) };
        format!("Use 1 (not {actual}) tabs for{name} indentation.")
    } else {
        format!("Use {width} (not {indentation}) spaces for{name} indentation.")
    };
    // When autocorrecting, a body inside an earlier offense's node is left to a later pass.
    let fix = if cx.fixes.autocorrect {
        let ranges = &mut cx.state::<State>().offense_ranges;
        let nested = ranges.iter().any(|&(rs, re)| range.0 >= rs && range.1 <= re);
        if !nested {
            ranges.push(range);
        }
        !nested
    } else {
        true
    };
    let set = helpers::alignment_corrector::Settings::of(cx);
    cx.add_fix(s.max(0) as usize, e.max(0) as usize, msg, |c| {
        if fix {
            helpers::alignment_corrector::correct(c, src, set, range.0, range.1, Some(node), delta, true);
        }
    });
}

fn style_align_with<'s>(cx: &Cx<'s, '_>) -> &'s str {
    cx.cfg.str("EnforcedStyleAlignWith").unwrap_or("start_of_line")
}

fn consistency_style(cx: &Cx) -> String {
    cx.config.for_cop("Layout/IndentationConsistency").str("EnforcedStyle").unwrap_or("normal").to_string()
}

/// `check_members(base, [body])`; `macro_scope` says whether the body's children are macros' scope.
fn check_members(cx: &mut Cx, base: usize, body: Option<&Body>, macro_scope: bool) {
    let ctx = Ctx { macro_scope };
    // select_check_member
    // `member.children.first`: a begin's first statement, or a lone `begin`/`(...)` body's
    // (other node types cannot start with a macro-scope send).
    let wrapper_first = match body {
        Some(Body::Single(n)) => match (n.as_begin_node(), n.as_parentheses_node()) {
            (Some(b), _) if b.rescue_clause().is_none() && b.ensure_clause().is_none() => {
                b.statements().and_then(|s| s.body().iter().next())
            }
            (_, Some(p)) => p.body().and_then(|b| b.as_statements_node()).and_then(|s| s.body().iter().next()),
            _ => None,
        },
        _ => None,
    };
    let first_modifier = match body {
        Some(Body::Begin(v)) if access_modifier(&v[0], macro_scope, false) => Some(Body::Child(&v[0])),
        _ => wrapper_first.filter(|f| access_modifier(f, macro_scope, false)).map(Body::Single),
    };
    let outdent = cx.config.for_cop("Layout/AccessModifierIndentation").str("EnforcedStyle") == Some("outdent");
    match (&first_modifier, outdent) {
        (Some(_), true) => {}
        (Some(m), false) => check_indentation(cx, base, Some(m), "normal", &ctx),
        (None, _) => check_indentation(cx, base, body, "normal", &ctx),
    }
    let Some(Body::Begin(children)) = body else { return };
    if consistency_style(cx) == "indented_internal_methods" {
        let mut previous: Option<usize> = None;
        for m in children {
            let special = bare_access_modifier(m, macro_scope)
                && matches!(cx.src.slice(span(m).0, span(m).1), "private" | "protected");
            if special {
                previous = Some(span(m).0);
            } else if let Some(p) = previous.take() {
                let b = Body::Child(m);
                check_indentation(cx, p, Some(&b), "indented_internal_methods", &ctx);
            }
        }
    } else {
        for m in children {
            if access_modifier(m, macro_scope, false) {
                continue;
            }
            let b = Body::Child(m);
            check_indentation(cx, base, Some(&b), "normal", &ctx);
        }
    }
}

/// `in_macro_scope?` of an if node (an `elsif` shares its `if`'s scope).
fn if_in_macro_scope(ancestors: &[Node], node: &Node) -> bool {
    let mut n = ancestors.len();
    let mut cur = span(node);
    while n > 0 {
        let Some(sub) = ancestors[n - 1].as_if_node().and_then(|i| i.subsequent()) else { break };
        if span(&sub) != cur {
            break;
        }
        n -= 1;
        cur = span(&ancestors[n]);
    }
    helpers::macro_scope::in_macro_scope_of_child(&ancestors[..n])
}

/// `on_if(node, base)` / `check_if`.
fn on_if(cx: &mut Cx, node: &Node, base: usize, macro_scope: bool) {
    if cx.state::<State>().ignored.contains(&span(node)) {
        return;
    }
    let ctx = Ctx { macro_scope };
    let (body, else_part) = if let Some(n) = node.as_if_node() {
        let Some(kw) = n.if_keyword_loc() else { return }; // ternary
        if kw.as_slice() != b"elsif" && n.end_keyword_loc().is_none() {
            return; // modifier
        }
        (n.statements(), n.subsequent())
    } else {
        let n = node.as_unless_node().unwrap();
        if n.end_keyword_loc().is_none() {
            return;
        }
        (n.statements(), n.else_clause().map(|e| e.as_node()))
    };
    let body = Body::of(body.map(|s| s.as_node()));
    check_indentation(cx, base, body.as_ref(), "normal", &ctx);
    let Some(e) = else_part.and_then(|e| e.as_else_node()) else { return };
    let eb = Body::of(e.statements().map(|s| s.as_node()));
    check_indentation(cx, lstart(e.else_keyword_loc()), eb.as_ref(), "normal", &ctx);
}

/// `on_while(node, base)`.
fn on_while(cx: &mut Cx, node: &Node, base: usize) {
    if cx.state::<State>().ignored.contains(&span(node)) {
        return;
    }
    let (kw, pred, stmts) = match node.as_while_node() {
        Some(w) => {
            if w.is_begin_modifier() {
                return;
            }
            (w.keyword_loc(), w.predicate(), w.statements())
        }
        None => {
            let u = node.as_until_node().unwrap();
            if u.is_begin_modifier() {
                return;
            }
            (u.keyword_loc(), u.predicate(), u.statements())
        }
    };
    let src = cx.src;
    if src.line_of(kw.start_offset()) != src.line_of(span(&pred).0) {
        return;
    }
    let body = Body::of(stmts.map(|s| s.as_node()));
    check_indentation(cx, base, body.as_ref(), "normal", &Ctx { macro_scope: false });
}

/// `on_block`: `call` is the block's call (or None for a lambda literal).
fn on_block(cx: &mut Cx, call: Option<&ruby_prism::CallNode>, body: Option<Node>, end: Location, macro_scope: bool) {
    let src = cx.src;
    if !begins_its_line(src, end.start_offset()) {
        return;
    }
    let mut base = end.start_offset();
    if style_align_with(cx) == "relative_to_receiver"
        && let Some(c) = call {
            let recv_last = c.receiver().map(|r| src.line_of(span(&r).1.saturating_sub(1).max(span(&r).0)));
            if let (Some(dot), Some(rl)) = (c.call_operator_loc(), recv_last) {
                if rl < src.line_of(dot.start_offset()) {
                    base = dot.start_offset();
                } else if let Some(sel) = c.message_loc().filter(|s| rl < src.line_of(s.start_offset())) {
                    base = sel.start_offset();
                }
            }
        }
    let body = Body::of(body);
    let ctx = Ctx { macro_scope };
    check_indentation(cx, base, body.as_ref(), "normal", &ctx);
    if consistency_style(cx) != "indented_internal_methods" {
        return;
    }
    let Some(Body::Begin(v)) = &body else { return };
    if v.iter().any(|c| bare_access_modifier(c, macro_scope)) {
        check_members(cx, end.start_offset(), body.as_ref(), macro_scope);
    }
}

/// CheckAssignment#check_assignment.
fn check_assignment(cx: &mut Cx, node: &Node) {
    let Some(rhs) = extract_rhs(node, true).and_then(first_part_of_call_chain) else { return };
    let src = cx.src;
    let end_cfg = cx.config.for_cop("Layout/EndAlignment");
    let end_style = end_cfg.str("EnforcedStyleAlignWith").unwrap_or("keyword");
    let variable = end_style != "keyword" && src.line_of(span(&rhs).0) <= src.line_of(span(node).0);
    let base = if variable { span(node).0 } else { span(&rhs).0 };
    match kind::of(&rhs) {
        kind::IF_NODE | kind::UNLESS_NODE => on_if(cx, &rhs, base, false),
        kind::WHILE_NODE | kind::UNTIL_NODE => on_while(cx, &rhs, base),
        _ => return,
    }
    cx.state::<State>().ignored.insert(span(&rhs));
}

/// `leftmost_modifier_of(node)`: climb parent sends. `i` indexes the call in `ancestors` + [call].
fn leftmost_modifier_start(cx: &Cx, call_start: usize) -> usize {
    let anc = cx.ancestors;
    let mut start = call_start;
    let mut n = anc.len();
    loop {
        // parent send: through an ArgumentsNode, or directly (we are its receiver)
        let p = match n.checked_sub(1).map(|i| kind::of(&anc[i])) {
            Some(kind::ARGUMENTS_NODE) => n.checked_sub(2),
            Some(kind::CALL_NODE) => n.checked_sub(1),
            _ => None,
        };
        let Some(p) = p.filter(|&p| anc[p].as_call_node().is_some_and(|c| !is_csend(&c))) else { return start };
        start = span(&anc[p]).0;
        n = p;
    }
}

impl Cop for IndentationWidth {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        static K: LazyLock<Vec<usize>> = LazyLock::new(|| {
            let mut k = KINDS.to_vec();
            k.extend([
                kind::BEGIN_NODE,
                kind::RESCUE_NODE,
                kind::FOR_NODE,
                kind::PARENTHESES_NODE,
                kind::SUPER_NODE,
                kind::FORWARDING_SUPER_NODE,
                kind::LAMBDA_NODE,
                kind::CLASS_NODE,
                kind::SINGLETON_CLASS_NODE,
                kind::MODULE_NODE,
                kind::DEF_NODE,
                kind::WHILE_NODE,
                kind::UNTIL_NODE,
                kind::CASE_NODE,
                kind::CASE_MATCH_NODE,
                kind::IF_NODE,
                kind::UNLESS_NODE,
            ]);
            k
        });
        &K
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let no_macro = Ctx { macro_scope: false };
        match kind::of(node) {
            kind::BEGIN_NODE => {
                let b = node.as_begin_node().unwrap();
                // parser order: kwbegin, then its ensure, then the rescue (with the else branch)
                if let (Some(_), Some(end)) = (b.begin_keyword_loc(), b.end_keyword_loc())
                    && begins_its_line(cx.src, end.start_offset()) {
                        let body = if b.rescue_clause().is_some() || b.ensure_clause().is_some() {
                            Some(Body::wrapped(node.as_begin_node().unwrap()))
                        } else {
                            b.statements().and_then(|s| s.body().iter().next()).map(Body::Single)
                        };
                        check_indentation(cx, end.start_offset(), body.as_ref(), "normal", &no_macro);
                    }
                if let Some(e) = b.ensure_clause() {
                    let body = Body::of(e.statements().map(|s| s.as_node()));
                    check_indentation(cx, lstart(e.ensure_keyword_loc()), body.as_ref(), "normal", &no_macro);
                }
                if let (Some(_), Some(e)) = (b.rescue_clause(), b.else_clause()) {
                    let body = Body::of(e.statements().map(|s| s.as_node()));
                    check_indentation(cx, lstart(e.else_keyword_loc()), body.as_ref(), "normal", &no_macro);
                }
            }
            kind::RESCUE_NODE => {
                let r = node.as_rescue_node().unwrap();
                let body = Body::of(r.statements().map(|s| s.as_node()));
                check_indentation(cx, lstart(r.keyword_loc()), body.as_ref(), "normal", &no_macro);
            }
            kind::FOR_NODE => {
                let f = node.as_for_node().unwrap();
                let body = Body::of(f.statements().map(|s| s.as_node()));
                check_indentation(cx, lstart(f.for_keyword_loc()), body.as_ref(), "normal", &no_macro);
            }
            kind::PARENTHESES_NODE => {
                let p = node.as_parentheses_node().unwrap();
                let src = cx.src;
                if !begins_its_line(src, p.closing_loc().start_offset()) {
                    return;
                }
                let open = p.opening_loc().start_offset();
                let line = src.line_of(open);
                let text = src.lines.get(line - 1).copied().unwrap_or("");
                let col = text.find(|c: char| !matches!(c, ' ' | '\t' | '\x0b' | '\x0c' | '\r')).unwrap_or(0);
                let first = p.body().and_then(|b| match b.as_statements_node() {
                    Some(s) => s.body().iter().next(),
                    None => Some(b),
                });
                let body = first.map(Body::Single);
                check_indentation(cx, src.line_start(line) + col, body.as_ref(), "normal", &no_macro);
            }
            kind::CALL_NODE => {
                let c = node.as_call_node().unwrap();
                if let Some(b) = c.block().and_then(|b| b.as_block_node()) {
                    let m = helpers::macro_scope::class_constructor(&c)
                        || helpers::macro_scope::in_macro_scope_of_child(cx.ancestors);
                    on_block(cx, Some(&c), b.body(), b.closing_loc(), m);
                }
                check_assignment(cx, node);
                // adjacent_def_modifier?: `(send nil? _ (any_def ...))`
                let args = send_args(&c);
                if c.receiver().is_some() || args.len() != 1 {
                    return;
                }
                let Some(def) = args[0].as_def_node() else { return };
                let def_style = cx.config.for_cop("Layout/DefEndAlignment");
                let base = if def_style.str("EnforcedStyleAlignWith") == Some("def") {
                    span(&args[0]).0
                } else {
                    leftmost_modifier_start(cx, span(node).0)
                };
                let body = Body::of(def.body());
                check_indentation(cx, base, body.as_ref(), "normal", &no_macro);
                cx.state::<State>().ignored.insert(span(&args[0]));
            }
            kind::SUPER_NODE | kind::FORWARDING_SUPER_NODE => {
                let block = match node.as_super_node() {
                    Some(s) => s.block().and_then(|b| b.as_block_node()),
                    None => node.as_forwarding_super_node().unwrap().block(),
                };
                let Some(b) = block else { return };
                let m = helpers::macro_scope::in_macro_scope_of_child(cx.ancestors);
                on_block(cx, None, b.body(), b.closing_loc(), m);
            }
            kind::LAMBDA_NODE => {
                let l = node.as_lambda_node().unwrap();
                let m = helpers::macro_scope::in_macro_scope_of_child(cx.ancestors);
                on_block(cx, None, l.body(), l.closing_loc(), m);
            }
            kind::CLASS_NODE | kind::SINGLETON_CLASS_NODE | kind::MODULE_NODE => {
                let (kw, body) = match kind::of(node) {
                    kind::CLASS_NODE => {
                        let n = node.as_class_node().unwrap();
                        (lstart(n.class_keyword_loc()), n.body())
                    }
                    kind::MODULE_NODE => {
                        let n = node.as_module_node().unwrap();
                        (lstart(n.module_keyword_loc()), n.body())
                    }
                    _ => {
                        let n = node.as_singleton_class_node().unwrap();
                        (lstart(n.class_keyword_loc()), n.body())
                    }
                };
                let body = Body::of(body);
                if body.as_ref().is_some_and(|b| cx.src.line_of(b.start()) == cx.src.line_of(kw)) {
                    return;
                }
                check_members(cx, kw, body.as_ref(), true);
            }
            kind::DEF_NODE => {
                if cx.state::<State>().ignored.contains(&span(node)) {
                    return;
                }
                let d = node.as_def_node().unwrap();
                let body = Body::of(d.body());
                check_indentation(cx, lstart(d.def_keyword_loc()), body.as_ref(), "normal", &no_macro);
            }
            kind::WHILE_NODE | kind::UNTIL_NODE => on_while(cx, node, span(node).0),
            kind::CASE_NODE => {
                let c = node.as_case_node().unwrap();
                let mut last = None;
                for w in c.conditions().iter() {
                    let Some(w) = w.as_when_node() else { continue };
                    let body = Body::of(w.statements().map(|s| s.as_node()));
                    check_indentation(cx, lstart(w.keyword_loc()), body.as_ref(), "normal", &no_macro);
                    last = Some(lstart(w.keyword_loc()));
                }
                let (Some(last), Some(e)) = (last, c.else_clause()) else { return };
                let body = Body::of(e.statements().map(|s| s.as_node()));
                check_indentation(cx, last, body.as_ref(), "normal", &no_macro);
            }
            kind::CASE_MATCH_NODE => {
                let c = node.as_case_match_node().unwrap();
                let mut last = None;
                for i in c.conditions().iter() {
                    let Some(i) = i.as_in_node() else { continue };
                    let body = Body::of(i.statements().map(|s| s.as_node()));
                    check_indentation(cx, lstart(i.in_loc()), body.as_ref(), "normal", &no_macro);
                    last = Some(lstart(i.in_loc()));
                }
                let (Some(last), Some(e)) = (last, c.else_clause()) else { return };
                let body = Body::of(e.statements().map(|s| s.as_node()));
                check_indentation(cx, last, body.as_ref(), "normal", &no_macro);
            }
            kind::IF_NODE | kind::UNLESS_NODE => {
                if is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                let m = if_in_macro_scope(cx.ancestors, node);
                on_if(cx, node, span(node).0, m);
            }
            _ => check_assignment(cx, node),
        }
    }
}


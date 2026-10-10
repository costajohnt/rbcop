use crate::prelude::*;
use ruby_prism::{BeginNode, Location};

pub const NAME: &str = "Layout/EmptyLinesAroundExceptionHandlingKeywords";
pub static COP: EmptyLinesAroundExceptionHandlingKeywords = EmptyLinesAroundExceptionHandlingKeywords;
pub struct EmptyLinesAroundExceptionHandlingKeywords;

/// A parser-gem `rescue`/`ensure` body: keyword locations plus what
/// `last_body_and_end_on_same_line?` compares with the `end` line.
struct Keywords {
    offsets: Vec<(usize, usize)>,
    last_body: usize,
}

fn from_begin(b: &BeginNode) -> Option<Keywords> {
    let span = |l: Location| (l.start_offset(), l.end_offset());
    let mut offsets = Vec::new();
    let mut rescue_last = None;
    if let Some(e) = b.ensure_clause() {
        offsets.push(span(e.ensure_keyword_loc()));
    }
    if let Some(mut r) = b.rescue_clause() {
        if let Some(e) = b.else_clause() {
            offsets.push(span(e.else_keyword_loc()));
            rescue_last = Some(e.else_keyword_loc().start_offset());
        }
        offsets.push(span(r.keyword_loc()));
        while let Some(n) = r.subsequent() {
            r = n;
            offsets.push(span(r.keyword_loc()));
        }
        rescue_last = rescue_last.or(Some(r.keyword_loc().start_offset()));
    }
    if offsets.is_empty() {
        return None;
    }
    // An ensure node compares its own last line; a rescue node its else / last resbody line.
    let last_body = if let Some(e) = b.ensure_clause() {
        e.statements().map_or(e.ensure_keyword_loc().start_offset(), |s| s.location().end_offset() - 1)
    } else {
        rescue_last?
    };
    Some(Keywords { offsets, last_body })
}

fn keywords(body: &Node) -> Option<Keywords> {
    if let Some(b) = body.as_begin_node().filter(|b| b.begin_keyword_loc().is_none()) {
        return from_begin(&b);
    }
    let m = body.as_rescue_modifier_node()?;
    let k = m.keyword_loc();
    Some(Keywords { offsets: vec![(k.start_offset(), k.end_offset())], last_body: k.start_offset() })
}

/// The parser body of a def/block: a single statement, or the implicit rescue/ensure BeginNode.
fn single(body: Option<Node>) -> Option<Node> {
    let body = body?;
    match body.as_statements_node() {
        Some(s) => {
            let mut it = s.body().iter();
            let first = it.next()?;
            it.next().is_none().then_some(first)
        }
        None => Some(body),
    }
}

fn is_itblock(cx: &Cx, block: &Option<Node>) -> bool {
    cx.config.target_ruby >= 3.4 && block.as_ref().is_some_and(|p| p.as_it_parameters_node().is_some())
}

impl Cop for EmptyLinesAroundExceptionHandlingKeywords {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::BLOCK_NODE, kind::LAMBDA_NODE, kind::BEGIN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let loc = node.location();
        // (body, start of the parser node, its `end`)
        let (body, start, end) = match kind::of(node) {
            kind::DEF_NODE => {
                let d = node.as_def_node().unwrap();
                (single(d.body()), loc.start_offset(), d.end_keyword_loc().map(|l| l.start_offset()))
            }
            kind::BLOCK_NODE => {
                let b = node.as_block_node().unwrap();
                if is_itblock(cx, &b.parameters()) {
                    return;
                }
                let start = cx.parent().map_or(loc.start_offset(), |p| p.location().start_offset());
                (single(b.body()), start, Some(b.closing_loc().start_offset()))
            }
            kind::LAMBDA_NODE => {
                let l = node.as_lambda_node().unwrap();
                if is_itblock(cx, &l.parameters()) {
                    return;
                }
                (single(l.body()), loc.start_offset(), Some(l.closing_loc().start_offset()))
            }
            _ => {
                let b = node.as_begin_node().unwrap();
                if b.begin_keyword_loc().is_none() {
                    return;
                }
                // kwbegin children.first: the rescue/ensure node, else the first statement.
                let first = if b.rescue_clause().is_some() || b.ensure_clause().is_some() {
                    Some(b.as_node())
                } else {
                    b.statements().and_then(|s| s.body().iter().next())
                };
                let kw = match first {
                    Some(f) if kind::of(&f) == kind::BEGIN_NODE => from_begin(&b),
                    Some(f) => keywords(&f),
                    None => None,
                };
                let Some(kw) = kw else { return };
                let end = b.end_keyword_loc().map(|l| l.start_offset());
                report(cx, kw, loc.start_offset(), end);
                return;
            }
        };
        let Some(kw) = body.as_ref().and_then(keywords) else { return };
        report(cx, kw, start, end);
    }
}

fn report(cx: &mut Cx, kw: Keywords, start: usize, end: Option<usize>) {
    let src = cx.src;
    let def_line = src.line_of(start);
    for (s, e) in kw.offsets {
        let line = src.line_of(s);
        if line == def_line {
            continue;
        }
        // Endless def: `body.parent.loc.end` is nil and RuboCop crashes.
        let Some(end) = end else { return };
        if src.line_of(kw.last_body) == src.line_of(end) {
            continue;
        }
        let keyword = src.slice(s, e);
        let after = format!("Extra empty line detected after the `{keyword}`.");
        let before = format!("Extra empty line detected before the `{keyword}`.");
        helpers::empty_lines_around_body::check_line(cx, "no_empty_lines", line as i64, &after, true);
        helpers::empty_lines_around_body::check_line(cx, "no_empty_lines", line as i64 - 2, &before, true);
    }
}

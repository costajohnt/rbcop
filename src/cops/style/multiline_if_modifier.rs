use crate::helpers::alignment::configured_indentation_width;
use crate::helpers::if_node;
use crate::prelude::*;

pub const NAME: &str = "Style/MultilineIfModifier";
pub static COP: MultilineIfModifier = MultilineIfModifier;
pub struct MultilineIfModifier;

#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

/// `line.sub(/^\s{n}/, indentation)` on one line from `each_line`.
fn reindent(line: &str, n: usize, indentation: &str) -> String {
    let mut chars = line.char_indices();
    let mut end = 0;
    for _ in 0..n {
        match chars.next() {
            Some((i, c)) if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c' | '\x0b') => end = i + c.len_utf8(),
            _ => return line.to_string(),
        }
    }
    format!("{indentation}{}", &line[end..])
}

/// The span whose lines decide `multiline?`: a block compares its `{`/`do` with its `}`/`end`.
fn multiline_span(body: &Node) -> (usize, usize) {
    let block = match body {
        Node::CallNode { .. } => body.as_call_node().unwrap().block(),
        Node::SuperNode { .. } => body.as_super_node().unwrap().block(),
        Node::ForwardingSuperNode { .. } => body.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
        Node::LambdaNode { .. } => {
            let l = body.as_lambda_node().unwrap();
            return (l.opening_loc().start_offset(), l.closing_loc().start_offset());
        }
        _ => None,
    };
    match block.and_then(|b| b.as_block_node()) {
        Some(b) => (b.opening_loc().start_offset(), b.closing_loc().start_offset()),
        None => (body.location().start_offset(), body.location().end_offset()),
    }
}

impl Cop for MultilineIfModifier {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        if cx.state::<Ignored>().0.iter().any(|&(a, b)| a <= start && end <= b) {
            return;
        }
        let Some(c) = if_node::cond(node) else { return };
        let Some(body) = c.body.as_ref() else { return };
        let (bs, be) = multiline_span(body);
        if !c.modifier() || cx.src.line_of(bs) == cx.src.line_of(be) {
            return;
        }
        let offset = " ".repeat(cx.src.column(start));
        let indentation = format!("{offset}{}", " ".repeat(configured_indentation_width(cx).max(0) as usize));
        let body_source = format!("{offset}{}", node_text(body));
        let indented: String = body_source
            .split_inclusive('\n')
            .map(|l| if l == "\n" { l.to_string() } else { reindent(l, offset.chars().count(), &indentation) })
            .collect();
        let replacement = format!("{} {}\n{indented}\n{offset}end", c.keyword, node_text(&c.condition));
        let msg = format!("Favor a normal {}-statement over a modifier clause in a multiline statement.", c.keyword);
        cx.add_fix(start, end, msg, |corr| corr.replace(start, end, &replacement));
        cx.state::<Ignored>().0.push((start, end));
    }
}

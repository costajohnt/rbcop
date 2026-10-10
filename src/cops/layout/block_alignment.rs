use crate::prelude::*;
use crate::source::Source;

pub const NAME: &str = "Layout/BlockAlignment";
pub static COP: BlockAlignment = BlockAlignment;
pub struct BlockAlignment;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Block,
    /// `send` (not csend) with its method name and whether the child below is its receiver
    Send { shovel: bool, index: bool, receiver_is_child: bool },
    /// assignment?; op_asgn / masgn carry their lhs range (find_lhs_node)
    Assign { lhs: Option<(usize, usize)> },
    Masgn { lhs: (usize, usize) },
    /// any_def, splat, and, or
    Target,
    Other,
}

/// A parser-gem node on the path from the block to the root.
#[derive(Clone, Copy)]
struct P {
    start: usize,
    end: usize,
    kind: Kind,
}

fn span(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

fn is_literal_block(b: Option<Node>) -> bool {
    b.is_some_and(|b| b.as_block_node().is_some())
}

/// Range of an op-assign's / masgn's lhs as parser has it.
fn lhs_range(node: &Node) -> Option<(usize, usize)> {
    let loc = |l: ruby_prism::Location| (l.start_offset(), l.end_offset());
    Some(match kind::of(node) {
        kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE => loc(node.as_local_variable_operator_write_node()?.name_loc()),
        kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE => {
            loc(node.as_instance_variable_operator_write_node()?.name_loc())
        }
        kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE => loc(node.as_class_variable_operator_write_node()?.name_loc()),
        kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE => loc(node.as_global_variable_operator_write_node()?.name_loc()),
        kind::CONSTANT_OPERATOR_WRITE_NODE => loc(node.as_constant_operator_write_node()?.name_loc()),
        kind::CONSTANT_PATH_OPERATOR_WRITE_NODE => {
            span(&node.as_constant_path_operator_write_node()?.target().as_node())
        }
        kind::CALL_OPERATOR_WRITE_NODE => {
            let n = node.as_call_operator_write_node()?;
            (node.location().start_offset(), n.message_loc()?.end_offset())
        }
        kind::INDEX_OPERATOR_WRITE_NODE => {
            let n = node.as_index_operator_write_node()?;
            (node.location().start_offset(), n.closing_loc().end_offset())
        }
        kind::MULTI_WRITE_NODE => {
            let n = node.as_multi_write_node()?;
            let targets: Vec<Node> =
                n.lefts().iter().chain(n.rest()).chain(n.rights().iter()).collect();
            let start = n.lparen_loc().map(|l| l.start_offset()).or(targets.first().map(|t| span(t).0))?;
            let end = n.rparen_loc().map(|l| l.end_offset()).or(targets.last().map(|t| span(t).1))?;
            (start, end)
        }
        _ => return None,
    })
}

fn classify(node: &Node, child: Option<(usize, usize)>) -> Kind {
    match kind::of(node) {
        kind::LOCAL_VARIABLE_WRITE_NODE
        | kind::INSTANCE_VARIABLE_WRITE_NODE
        | kind::CLASS_VARIABLE_WRITE_NODE
        | kind::GLOBAL_VARIABLE_WRITE_NODE
        | kind::CONSTANT_WRITE_NODE
        | kind::CONSTANT_PATH_WRITE_NODE
        | kind::LOCAL_VARIABLE_OR_WRITE_NODE
        | kind::LOCAL_VARIABLE_AND_WRITE_NODE
        | kind::INSTANCE_VARIABLE_OR_WRITE_NODE
        | kind::INSTANCE_VARIABLE_AND_WRITE_NODE
        | kind::CLASS_VARIABLE_OR_WRITE_NODE
        | kind::CLASS_VARIABLE_AND_WRITE_NODE
        | kind::GLOBAL_VARIABLE_OR_WRITE_NODE
        | kind::GLOBAL_VARIABLE_AND_WRITE_NODE
        | kind::CONSTANT_OR_WRITE_NODE
        | kind::CONSTANT_AND_WRITE_NODE
        | kind::CONSTANT_PATH_OR_WRITE_NODE
        | kind::CONSTANT_PATH_AND_WRITE_NODE
        | kind::CALL_OR_WRITE_NODE
        | kind::CALL_AND_WRITE_NODE
        | kind::INDEX_OR_WRITE_NODE
        | kind::INDEX_AND_WRITE_NODE => Kind::Assign { lhs: None },
        kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE
        | kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE
        | kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE
        | kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE
        | kind::CONSTANT_OPERATOR_WRITE_NODE
        | kind::CONSTANT_PATH_OPERATOR_WRITE_NODE
        | kind::CALL_OPERATOR_WRITE_NODE
        | kind::INDEX_OPERATOR_WRITE_NODE => Kind::Assign { lhs: lhs_range(node) },
        kind::MULTI_WRITE_NODE => match lhs_range(node) {
            Some(lhs) => Kind::Masgn { lhs },
            None => Kind::Other,
        },
        kind::DEF_NODE | kind::SPLAT_NODE | kind::AND_NODE | kind::OR_NODE => Kind::Target,
        kind::CALL_NODE => {
            let c = node.as_call_node().unwrap();
            // MethodDispatchNode#assignment? (setter_method?: `a.b = x`, `a[i] = x`, also with `&.`)
            if c.equal_loc().is_some() {
                return Kind::Assign { lhs: None };
            }
            if is_csend(&c) {
                return Kind::Other;
            }
            let name = method_name(&c);
            let receiver_is_child = c.receiver().is_some_and(|r| Some(span(&r)) == child);
            Kind::Send { shovel: name == "<<", index: name == "[]", receiver_is_child }
        }
        _ => Kind::Other,
    }
}

/// The block (X) and its parser-gem ancestors, nearest first.
fn lineage(x: &Node, ancestors: &[Node]) -> Vec<P> {
    let (s, e) = span(x);
    let mut out = vec![P { start: s, end: e, kind: Kind::Block }];
    let mut child = Some((s, e));
    let mut i = ancestors.len();
    while i > 0 {
        i -= 1;
        let a = &ancestors[i];
        let (s, e) = span(a);
        match frame(a) {
            Frame::Transparent => continue,
            _ if kind::of(a) == kind::BLOCK_NODE => {
                // parser block = owning call + block; we came from inside the block, so the
                // owner's `send` is not on our path.
                let start = i.checked_sub(1).map_or(s, |j| span(&ancestors[j]).0);
                out.push(P { start, end: e, kind: Kind::Block });
                i = i.saturating_sub(1);
                child = Some((start, e));
                continue;
            }
            _ => {
                let owns_block = match kind::of(a) {
                    kind::CALL_NODE => is_literal_block(a.as_call_node().unwrap().block()),
                    kind::SUPER_NODE => is_literal_block(a.as_super_node().unwrap().block()),
                    kind::FORWARDING_SUPER_NODE => {
                        a.as_forwarding_super_node().unwrap().block().is_some()
                    }
                    _ => false,
                };
                let send_end = a.as_call_node().map_or(e, |c| send_range(&c).1);
                out.push(P { start: s, end: send_end, kind: classify(a, child) });
                if owns_block {
                    out.push(P { start: s, end: e, kind: Kind::Block });
                }
            }
        }
        child = Some((s, e));
    }
    out
}

/// Index into the lineage of block_end_align_target.
fn align_target(src: &Source, lin: &[P]) -> usize {
    for j in 1..lin.len() {
        let (node, parent) = (&lin[j - 1], &lin[j]);
        let disqualified =
            src.line_of(parent.start) != src.line_of(node.start) && !matches!(parent.kind, Kind::Masgn { .. });
        let target = match parent.kind {
            Kind::Assign { .. } | Kind::Masgn { .. } | Kind::Target => true,
            Kind::Send { shovel, index, receiver_is_child } => shovel || (receiver_is_child && !index),
            _ => false,
        };
        if disqualified || !target {
            return j - 1;
        }
    }
    lin.len() - 1
}

/// find_lhs_node on lineage[k]: its range and the index where its parser ancestors start.
fn find_lhs(lin: &[P], k: usize) -> ((usize, usize), usize) {
    match lin[k].kind {
        Kind::Assign { lhs: Some(r) } | Kind::Masgn { lhs: r } => (r, k),
        _ => ((lin[k].start, lin[k].end), k + 1),
    }
}

/// Ruby `=~ /\S/` on a line: index (in chars) of the first non-whitespace character.
fn first_non_space(line: &str) -> Option<(usize, usize)> {
    line.char_indices()
        .enumerate()
        .find(|(_, (_, c))| !matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
        .map(|(ci, (bi, _))| (ci, bi))
}

fn first_source_line<'a>(src: &Source<'a>, (start, end): (usize, usize)) -> &'a str {
    let s = src.slice(start, end);
    s.split('\n').next().unwrap_or("")
}

impl Cop for BlockAlignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::LAMBDA_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (do/{ , end/}, send-node arguments, selector || send range)
        let (block, args, selector): (Option<Node>, Vec<Node>, (usize, usize)) = match kind::of(node) {
            kind::CALL_NODE => {
                let c = node.as_call_node().unwrap();
                let sel = c.message_loc().map_or(send_range(&c), |m| (m.start_offset(), m.end_offset()));
                (c.block(), send_args(&c), sel)
            }
            kind::SUPER_NODE => {
                let c = node.as_super_node().unwrap();
                let k = c.keyword_loc();
                (c.block(), c.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default(), (k.start_offset(), k.end_offset()))
            }
            kind::FORWARDING_SUPER_NODE => {
                let c = node.as_forwarding_super_node().unwrap();
                (c.block().map(|b| b.as_node()), Vec::new(), (0, 0))
            }
            _ => {
                let l = node.as_lambda_node().unwrap();
                let (o, c) = (l.opening_loc(), l.closing_loc());
                self.check(cx, node, (o.start_offset(), o.end_offset()), (c.start_offset(), c.end_offset()), &[], (0, 0));
                return;
            }
        };
        let Some(b) = block.and_then(|b| b.as_block_node()) else { return };
        let (o, c) = (b.opening_loc(), b.closing_loc());
        self.check(cx, node, (o.start_offset(), o.end_offset()), (c.start_offset(), c.end_offset()), &args, selector);
    }
}

impl BlockAlignment {
    fn check(&self, cx: &mut Cx, node: &Node, do_loc: (usize, usize), end_loc: (usize, usize), args: &[Node], selector: (usize, usize)) {
        let src = cx.src;
        if !src.begins_its_line(end_loc.0) {
            return;
        }
        let style = cx.cfg.str("EnforcedStyleAlignWith").unwrap_or("either");
        let lin = lineage(node, cx.ancestors);
        let k = align_target(src, &lin);
        let (block_start, anc) = find_lhs(&lin, k);
        // start_for_line_node: the outermost ancestor starting on the same line, then its lhs.
        let line_start = match lin[anc..].iter().rposition(|p| src.line_of(p.start) == src.line_of(block_start.0)) {
            Some(j) => find_lhs(&lin, anc + j).0,
            None => block_start,
        };
        let start = if style == "start_of_line" { line_start } else { block_start };
        let end_col = src.column(end_loc.0);
        if src.column(start.0) == end_col && style != "start_of_block" {
            return;
        }

        // compute_do_source_line_column
        let do_line = src.line_of(do_loc.0);
        let line_text = |l: usize| src.lines.get(l - 1).copied().unwrap_or("");
        let (_, first_byte) = first_non_space(line_text(do_line)).unwrap_or((0, 0));
        let first_char_pos = src.line_start(do_line) + first_byte;
        let anchor = if self.inside_parentheses(src, span(node).0, first_char_pos)
            && args.iter().any(|a| {
                let (s, e) = span(a);
                s <= first_char_pos && first_char_pos < e
            }) {
            selector
        } else {
            do_loc
        };
        let anchor_line = src.line_of(anchor.0);
        let anchor_text = line_text(anchor_line);
        let Some((indent, indent_byte)) = first_non_space(anchor_text) else { return };
        let mut permitted = vec![indent];
        if style == "either" {
            permitted.extend(first_non_space(line_text(do_line)).map(|(c, _)| c));
        }
        if permitted.contains(&end_col) && style != "start_of_line" {
            return;
        }
        let do_src = (&anchor_text[indent_byte..], anchor_line, indent);
        let start_src = (first_source_line(src, start), src.line_of(start.0), src.column(start.0));
        let fmt = |(s, l, c): (&str, usize, usize)| format!("`{s}` at {l}, {c}");
        let current = (first_source_line(src, end_loc), src.line_of(end_loc.0), end_col);
        let prefer = if style == "start_of_block" { do_src } else { start_src };
        let alt = if style != "either" || (start_src.1 == do_src.1 && start_src.2 == do_src.2) {
            String::new()
        } else {
            format!(" or {}", fmt(do_src))
        };
        let msg = format!("{} is not aligned with {}{alt}.", fmt(current), fmt(prefer));
        // The corrector moves `end` to start_for_line_node's column (start_of_block: the anchor
        // line's indentation); nothing to do when it is already there.
        let correctable = style == "start_of_block" || src.column(line_start.0) != end_col;
        cx.add_with(end_loc.0, end_loc.1, msg, correctable);
    }

    /// inside_parentheses?: more `(`/`[` than `)`/`]` tokens between the block start and `pos`
    /// (parser types: tLPAREN/tLPAREN2 and tLBRACK/tLBRACK2 open; tLPAREN_ARG does not).
    fn inside_parentheses(&self, src: &Source, from: usize, pos: usize) -> bool {
        let mut depth = 0i64;
        for t in src.tokens().iter().filter(|t| t.start >= from && t.start < pos) {
            match t.kind {
                sys::PM_TOKEN_PARENTHESIS_LEFT | sys::PM_TOKEN_BRACKET_LEFT | sys::PM_TOKEN_BRACKET_LEFT_ARRAY => {
                    depth += 1
                }
                sys::PM_TOKEN_PARENTHESIS_RIGHT | sys::PM_TOKEN_BRACKET_RIGHT => depth -= 1,
                _ => {}
            }
        }
        depth > 0
    }
}

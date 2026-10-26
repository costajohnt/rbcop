use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use crate::source::Source;
use ruby_prism::{BlockNode, CallNode};

pub const NAME: &str = "Style/CombinableLoops";
pub static COP: CombinableLoops = CombinableLoops;
pub struct CombinableLoops;

type R = (usize, usize);

/// Corrector edits: a removal (empty text) or an insertion before the range.
type Edits = Vec<(R, &'static str)>;

fn range(n: &Node) -> R {
    (n.location().start_offset(), n.location().end_offset())
}

fn loc(l: &ruby_prism::Location) -> R {
    (l.start_offset(), l.end_offset())
}

/// DirectiveComment#start_with_marker?
static MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A#[ \t\n\x0B\x0C\r]*rubocop[ \t\n\x0B\x0C\r]*:").unwrap());

/// A statement that is a parser `block`/`numblock`/`itblock`: a call with a literal block.
struct Blk<'pr> {
    call: CallNode<'pr>,
    block: BlockNode<'pr>,
}

fn as_block<'pr>(n: &Node<'pr>) -> Option<Blk<'pr>> {
    let call = n.as_call_node()?;
    let block = call.block()?.as_block_node()?;
    Some(Blk { call, block })
}

/// Parser-node `==` of two optional nodes.
fn same_opt(src: &Source, a: Option<Node>, b: Option<Node>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => helpers::node_equality::same(src, range(&a), range(&b)),
        _ => false,
    }
}

/// Block `arguments` as compared by `==`: None for `[]` (numblock/itblock), Some(range) for an
/// `args` node (empty range when there are no parameters).
fn block_args(b: &BlockNode) -> Option<Option<R>> {
    match b.parameters() {
        None => Some(None),
        Some(p) => p.as_block_parameters_node().map(|bp| match bp.parameters() {
            None if bp.locals().iter().next().is_none() => None,
            _ => Some(range(&p)),
        }),
    }
}

/// `argument_list` of a numblock (`_1`.._n`) or itblock (`it`); None for an ordinary block.
fn implicit_args(b: &BlockNode) -> Option<u8> {
    let p = b.parameters()?;
    if let Some(n) = p.as_numbered_parameters_node() {
        return Some(n.maximum());
    }
    p.as_it_parameters_node().map(|_| 0)
}

/// A `rescue`/`ensure` body (parser's `rescue` or `ensure` node).
fn rescue_or_ensure(body: &Node) -> bool {
    body.as_begin_node().is_some_and(|b| {
        b.begin_keyword_loc().is_none() && (b.rescue_clause().is_some() || b.ensure_clause().is_some())
    })
}

fn body_first_line(src: &Source, body: &Node) -> usize {
    src.line_of(body_range(body).0)
}

/// `same_collection_looping_block?(node, sibling)`
fn same_collection(src: &Source, node: &Blk, sibling: &Blk) -> bool {
    if method_name(&node.call) != method_name(&sibling.call) || !same_opt(src, node.call.receiver(), sibling.call.receiver()) {
        return false;
    }
    let (a, b) = (call_args(&node.call), call_args(&sibling.call));
    a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| helpers::node_equality::same(src, range(x), range(y)))
}

/// `combinable_looping_blocks?(node, sibling)`
fn combinable_looping(src: &Source, node: &Blk, sibling: &Blk) -> bool {
    same_collection(src, node, sibling) && node.block.body().is_some() && sibling.block.body().is_some()
}

/// `loop_opening` of a block: its `|args|`, or `{`/`do`.
fn block_opening(b: &Blk) -> R {
    match b.block.parameters().filter(|p| p.as_block_parameters_node().is_some()) {
        Some(p) => range(&p),
        None => loc(&b.block.opening_loc()),
    }
}

/// `directive_after_opening?`: a directive comment on the opening's line, before the body.
fn directive_after_opening(src: &Source, opening: R, body: &Node) -> bool {
    let line = src.line_of(opening.0);
    if line == body_first_line(src, body) {
        return false;
    }
    src.comment_on_line(line).is_some_and(|c| MARKER.is_match(&src.text[c.start..c.end]))
}

/// `combinable_blocks?(node, sibling)`
fn combinable_blocks(src: &Source, node: &Blk, sibling: &Blk) -> bool {
    let same_args = match (block_args(&node.block), block_args(&sibling.block)) {
        (Some(None), Some(None)) | (None, None) => true,
        (Some(Some(a)), Some(Some(b))) => helpers::node_equality::same(src, a, b),
        _ => false,
    };
    let (Some(nb), Some(sb)) = (node.block.body(), sibling.block.body()) else { return false };
    same_args
        && implicit_args(&node.block) == implicit_args(&sibling.block)
        && !directive_after_opening(src, block_opening(node), &nb)
        && !rescue_or_ensure(&nb)
        && !rescue_or_ensure(&sb)
}

const MSG: &str = "Combine this loop with the previous loop.";

impl Cop for CombinableLoops {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let stmts: Vec<Node> = node.as_statements_node().unwrap().body().iter().collect();
        if stmts.len() < 2 {
            return;
        }
        // `begin a; b; end` without rescue/ensure is `(kwbegin a b)`: no parser `begin` parent.
        if let Some(b) = cx.ancestors.last().and_then(|a| a.as_begin_node())
            && b.begin_keyword_loc().is_some()
            && b.rescue_clause().is_none()
            && b.ensure_clause().is_none()
        {
            return;
        }
        let src = cx.src;
        let text = src.text;
        let blocks: Vec<Option<Blk>> = stmts.iter().map(as_block).collect();
        let enabled = |r: R| {
            !crate::directives::suppressed(&cx.facts.disabled, cx.cop, src.line_of(r.0), src.line_of(r.1))
        };
        // closing_with_preceding_space: the line break before the closing stays when the next
        // loop starts on its line.
        let closing_removal = |closing: R, next: &Node| {
            let sep = src.line_of(closing.0) != src.line_of(range(next).0);
            let opts = Surrounding { side: Side::Left, newlines: sep, whitespace: sep, ..Default::default() };
            range_with_surrounding_space(text, closing.0, closing.1, opts)
        };
        // opening_with_following_space
        let opening_removal = |start: usize, opening: R| {
            let opts = Surrounding { side: Side::Right, whitespace: true, ..Default::default() };
            range_with_surrounding_space(text, start, opening.1, opts)
        };
        let mut fixes: Vec<(R, Option<Edits>)> = Vec::new();
        for i in 1..stmts.len() {
            let (left, cur) = (&stmts[i - 1], &stmts[i]);
            if let (Some(lb), Some(cb)) = (&blocks[i - 1], &blocks[i]) {
                let name = method_name(&cb.call);
                if !(name.starts_with("each") || name.ends_with("_each")) || !combinable_looping(src, cb, lb) {
                    continue;
                }
                if !combinable_blocks(src, cb, lb) {
                    fixes.push((range(cur), None));
                    continue;
                }
                let mut edits = vec![
                    (closing_removal(loc(&lb.block.closing_loc()), cur), ""),
                    (opening_removal(range(cur).0, block_opening(cb)), ""),
                ];
                // correct_end_of_block
                let right_combined = blocks.get(i + 1).and_then(|r| r.as_ref()).is_some_and(|rb| {
                    enabled(range(&stmts[i + 1])) && combinable_looping(src, rb, cb) && combinable_blocks(src, rb, cb)
                });
                if !right_combined {
                    // first_combined_block
                    let mut first = i - 1;
                    while first > 0
                        && let (Some(fb), Some(pb)) = (&blocks[first], &blocks[first - 1])
                        && enabled(range(&stmts[first]))
                        && combinable_looping(src, fb, pb)
                        && combinable_blocks(src, fb, pb)
                    {
                        first -= 1;
                    }
                    let braces = blocks[first].as_ref().unwrap().block.opening_loc().as_slice() == b"{";
                    let end = range(cur).1;
                    edits.push((loc(&cb.block.closing_loc()), ""));
                    edits.push(((end, end), if braces { "}" } else { " end" }));
                }
                fixes.push((range(cur), Some(edits)));
            } else if let (Some(lf), Some(cf)) = (left.as_for_node(), cur.as_for_node()) {
                if !helpers::node_equality::same(src, range(&lf.collection()), range(&cf.collection())) {
                    continue;
                }
                let (Some(_), Some(cbody)) = (lf.statements(), cf.statements()) else { continue };
                let opening = cf.do_keyword_loc().map_or_else(|| range(&cf.collection()), |l| loc(&l));
                let combinable = helpers::node_equality::same(src, range(&lf.index()), range(&cf.index()))
                    && !directive_after_opening(src, opening, &cbody.as_node());
                let edits = combinable.then(|| {
                    vec![
                        (closing_removal(loc(&lf.end_keyword_loc()), cur), ""),
                        (opening_removal(range(cur).0, opening), ""),
                    ]
                });
                fixes.push((range(cur), edits));
            }
        }
        for ((s, e), edits) in fixes {
            cx.add_fix(s, e, MSG, |c| {
                for ((a, b), t) in edits.into_iter().flatten() {
                    if t.is_empty() {
                        c.remove(a, b);
                    } else {
                        c.insert_before(a, b, t);
                    }
                }
            });
        }
    }
}

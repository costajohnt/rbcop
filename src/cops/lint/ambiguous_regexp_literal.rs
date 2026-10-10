use crate::prelude::*;
use helpers::add_parentheses::{self as parens, ParenFix};

pub const NAME: &str = "Lint/AmbiguousRegexpLiteral";
pub static COP: AmbiguousRegexpLiteral = AmbiguousRegexpLiteral;
pub struct AmbiguousRegexpLiteral;

const MSG: &str = "Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.";

/// Regexp start offsets with the parenthesize plan of their offense node, in pre-order.
#[derive(Default)]
struct State(Vec<(usize, Option<ParenFix>)>);

fn is_regexp(n: &Node) -> bool {
    matches!(kind::of(n), kind::REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE)
}

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

impl Cop for AmbiguousRegexpLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::REGULAR_EXPRESSION_NODE, kind::INTERPOLATED_REGULAR_EXPRESSION_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let ancestors = cx.ancestors;
        let regexp = range(node);
        // Parser-gem ancestors, nearest first (indexes into `ancestors`).
        // A MatchWriteNode and its `=~` call are one parser `match_with_lvasgn`.
        let chain: Vec<usize> = parser_ancestor_ranges(ancestors)
            .into_iter()
            .map(|(i, _, _)| i)
            .filter(|&i| kind::of(&ancestors[i]) != kind::MATCH_WRITE_NODE)
            .collect();
        // `/re/ =~ x` is parser `match_with_lvasgn`, not a send.
        let match_with_lvasgn = |c: &ruby_prism::CallNode| {
            method_name(c) == "=~" && !is_csend(c) && c.receiver().is_some_and(|r| is_regexp(&r))
        };
        let call = |i: usize| ancestors[i].as_call_node().filter(|c| !match_with_lvasgn(c));
        let is_send = |i: usize| call(i).is_some_and(|c| !is_csend(&c));
        // rubocop-ast Node#receiver: a send's receiver, or a block's send's receiver.
        let receiver = |i: usize| -> Option<Node> {
            match kind::of(&ancestors[i]) {
                kind::CALL_NODE => call(i).and_then(|c| c.receiver()),
                kind::BLOCK_NODE if i > 0 => ancestors[i - 1].as_call_node().and_then(|c| c.receiver()),
                _ => None,
            }
        };
        let first_argument_is_regexp =
            |i: usize| is_send(i) && send_args(&call(i).unwrap()).first().is_some_and(is_regexp);
        let Some(mut at) = chain.first().map(|_| 0) else { return };
        // find_offense_node
        while !first_argument_is_regexp(chain[at]) && at + 1 < chain.len() {
            let (node_i, parent_i) = (chain[at], chain[at + 1]);
            let up = (is_send(parent_i) && receiver(node_i).is_some())
                || (at + 2 < chain.len()
                    && receiver(parent_i)
                        .and_then(|r| r.as_call_node().and_then(|c| c.receiver()))
                        .is_some_and(|rr| range(&rr) == regexp));
            if !up {
                break;
            }
            at += 1;
        }
        let target = &ancestors[chain[at]];
        let fix = if target.as_call_node().is_some_and(|c| match_with_lvasgn(&c)) {
            let (start, end) = range(target);
            Some(ParenFix::Wrap { start, end })
        } else {
            parens::plan(target, cx.src.text)
        };
        cx.state::<State>().0.push((regexp.0, fix));
    }

    fn on_file(&self, cx: &mut Cx) {
        let warnings = helpers::parse_warnings::warnings(cx.src);
        let state = std::mem::take(cx.state::<State>());
        // ponytail: below Ruby 3.0 RuboCop wants parser's :ambiguous_literal, which Prism never reports.
        for w in warnings.iter().filter(|w| w.kind == "ambiguous_slash") {
            // A regexp RuboCop cannot find (or whose corrector raises) loses the offense.
            let Some((_, Some(fix))) = state.0.iter().find(|(s, _)| *s == w.start) else { continue };
            cx.add_fix(w.start, w.end, MSG, |c| parens::apply(c, fix));
        }
    }
}

use crate::prelude::*;
use helpers::add_parentheses::{self as parens, ParenFix};

pub const NAME: &str = "Lint/AmbiguousOperator";
pub static COP: AmbiguousOperator = AmbiguousOperator;
pub struct AmbiguousOperator;

/// Nodes find_offense_node_by may pick, in pre-order: (start, the parenthesize plan, first-argument
/// source for sends / None for splat-like nodes).
#[derive(Default)]
struct State {
    splats: Vec<(usize, Option<ParenFix>)>,
    sends: Vec<(usize, String, Option<ParenFix>)>,
}

use helpers::parser_parent::parser_parent_index as parser_parent;

impl Cop for AmbiguousOperator {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::SPLAT_NODE, kind::BLOCK_ARGUMENT_NODE, kind::ASSOC_SPLAT_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let text = cx.src.text;
        let start = node.location().start_offset();
        if let Some(call) = node.as_call_node() {
            if is_csend(&call) {
                return;
            }
            let Some(first) = send_args(&call).into_iter().next() else { return };
            let fl = first.location();
            let source = text[fl.start_offset()..fl.end_offset()].chars().take(2).collect();
            let fix = parens::plan(node, text);
            cx.state::<State>().sends.push((fl.start_offset(), source, fix));
            return;
        }
        let ancestors = cx.ancestors;
        let Some(mut p) = parser_parent(ancestors, ancestors.len()) else { return };
        if kind::of(node) == kind::ASSOC_SPLAT_NODE {
            let Some(gp) = parser_parent(ancestors, p) else { return };
            p = gp;
        }
        let fix = parens::plan(&ancestors[p], text);
        cx.state::<State>().splats.push((start, fix));
    }

    fn on_file(&self, cx: &mut Cx) {
        let warnings = helpers::parse_warnings::warnings(cx.src);
        let state = cx.state::<State>();
        let state = std::mem::take(state);
        for w in warnings {
            let Some(prefix) = helpers::parse_warnings::ambiguous_prefix(w.kind) else { continue };
            let found = match state.splats.iter().find(|(s, _)| *s == w.start) {
                Some((_, fix)) => Some(fix),
                None => state
                    .sends
                    .iter()
                    .find(|(s, src, _)| *s == w.start && src.starts_with(prefix))
                    .map(|(_, _, fix)| fix),
            };
            let Some(fix) = found else { continue };
            let (actual, possible) = match cx.src.slice(w.start, w.end) {
                "+" => ("positive number", "an addition"),
                "-" => ("negative number", "a subtraction"),
                "*" => ("splat", "a multiplication"),
                "&" => ("block", "a binary AND"),
                "**" => ("keyword splat", "an exponent"),
                _ => continue,
            };
            let operator = cx.src.slice(w.start, w.end);
            let msg = format!(
                "Ambiguous {actual} operator. Parenthesize the method arguments if it's surely a {actual} operator, or add a whitespace to the right of the `{operator}` if it should be {possible}."
            );
            // Without a plan RuboCop's corrector raises and the offense is lost.
            if let Some(f) = fix {
                cx.add_fix(w.start, w.end, msg, |c| parens::apply(c, f));
            }
        }
    }
}

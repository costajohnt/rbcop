use crate::helpers::const_name::const_name;
use crate::helpers::parser_parent::{is_operator_keyword, is_ternary, parser_parent};
use crate::prelude::*;
use helpers::select_by::plain_send;

pub const NAME: &str = "Style/RaiseArgs";
pub static COP: RaiseArgs = RaiseArgs;
pub struct RaiseArgs;

impl Cop for RaiseArgs {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if call.receiver().is_some() || !matches!(method, "raise" | "fail") {
            return;
        }
        let args = send_args(&call);
        // requires_parens?(node.parent)
        let parens = parser_parent(cx.ancestors).is_some_and(|p| {
            is_operator_keyword(p)
                || is_ternary(p)
                || matches!(kind::of(p), kind::CALL_NODE | kind::SUPER_NODE | kind::YIELD_NODE | kind::ARRAY_NODE | kind::ASSOC_NODE)
                || p.as_def_node().is_some_and(|d| d.equal_loc().is_some())
        });
        // inside_corrected_raise?: an ancestor send is a raise this cop already corrected.
        let inside_corrected = {
            let ancestors = cx.ancestors;
            let ignored = &cx.state::<Ignored>().0;
            ancestors.iter().any(|a| {
                a.as_call_node().is_some_and(|c| !is_csend(&c) && ignored.contains(&send_range(&c)))
            })
        };
        // parenthesize?
        let parens = parens || call.opening_loc().is_some_and(|o| o.as_slice() == b"(");
        let node_parenthesized = call.opening_loc().is_some_and(|o| o.as_slice() == b"(");
        let (s, e) = send_range(&call);
        let wrap = |arguments: String| {
            if parens { format!("{method}({arguments})") } else { format!("{method} {arguments}") }
        };
        if cx.cfg.str("EnforcedStyle") == Some("compact") {
            if args.len() <= 1 {
                return cx.correct_style_detected();
            }
            let exception = &args[0];
            if plain_send(exception).is_some_and(|c| call_args(&c).first().is_some_and(is_hash)) {
                return;
            }
            // `exception.arguments?` (a block pass included)
            if new_call(exception).is_some_and(|(c, _)| c.arguments().is_some() || c.block().is_some()) {
                return;
            }
            // exception_class_node
            let (class_end, class_src) = match new_call(exception) {
                Some((_, r)) => (r.location().end_offset(), node_text(&r)),
                None => (exception.location().end_offset(), node_text(exception)),
            };
            let fix = (args.len() == 2).then(|| {
                let message = &args[1];
                if node_parenthesized && has_heredoc(message) {
                    // correct_heredoc_exploded_to_compact: the heredoc body stays in place.
                    let m = message.location();
                    vec![
                        (class_end, m.start_offset(), ".new(".to_string()),
                        (m.end_offset(), m.end_offset(), ")".to_string()),
                    ]
                } else {
                    vec![(s, e, wrap(format!("{class_src}.new({})", node_text(message))))]
                }
            });
            let msg = format!("Provide an exception object as an argument to `{method}`.");
            if cx.block_runs(s, e) {
                cx.opposite_style_detected();
                if !inside_corrected && fix.is_some() {
                    cx.state::<Ignored>().0.push((s, e));
                }
            }
            return cx.add_fix(s, e, msg, |c| {
                for (a, b, t) in fix.iter().flatten().filter(|_| !inside_corrected) {
                    if a == b {
                        c.insert_after(*a, *b, t);
                    } else {
                        c.replace(*a, *b, t);
                    }
                }
            });
        }
        let [first] = args.as_slice() else { return cx.correct_style_detected() };
        let Some(new) = plain_send(first) else { return };
        let Some(recv) = new.receiver().filter(|_| method_name(&new) == "new") else { return };
        let new_args = send_args(&new);
        let acceptable = match new_args.as_slice() {
            [] => false,
            [arg] => matches!(
                kind::of(arg),
                kind::HASH_NODE | kind::KEYWORD_HASH_NODE | kind::SPLAT_NODE | kind::FORWARDING_ARGUMENTS_NODE
            ),
            _ => true,
        };
        if acceptable {
            return;
        }
        if const_name(&recv).is_some_and(|n| cx.cfg.strings("AllowedCompactTypes").contains(&n)) {
            return;
        }
        // heredoc_new_argument?: rewrite around the heredoc so its body stays in place.
        let fix = match new_args.as_slice() {
            [arg] if has_heredoc(arg) && (node_parenthesized || !parens) => {
                let mut edits = vec![(recv.location().end_offset(), arg.location().start_offset(), ", ".to_string())];
                if let Some(close) = new.closing_loc() {
                    edits.push((close.start_offset(), close.end_offset(), String::new()));
                }
                edits
            }
            _ => {
                let arguments =
                    std::iter::once(node_text(&recv)).chain(new_args.first().map(node_text)).collect::<Vec<_>>();
                vec![(s, e, wrap(arguments.join(", ")))]
            }
        };
        let msg = format!("Provide an exception class and message as arguments to `{method}`.");
        if cx.block_runs(s, e) {
            cx.opposite_style_detected();
            if !inside_corrected {
                cx.state::<Ignored>().0.push((s, e));
            }
        }
        cx.add_fix(s, e, msg, |c| {
            for (a, b, t) in fix.iter().filter(|_| !inside_corrected) {
                c.replace(*a, *b, t);
            }
        });
    }
}

/// `ignore_node`d (corrected) raise calls.
#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

fn is_hash(node: &Node) -> bool {
    matches!(kind::of(node), kind::HASH_NODE | kind::KEYWORD_HASH_NODE)
}

/// `use_new_method?`: a `send` of `new` with a receiver, and that receiver.
fn new_call<'pr>(node: &Node<'pr>) -> Option<(ruby_prism::CallNode<'pr>, Node<'pr>)> {
    let call = plain_send(node).filter(|c| method_name(c) == "new")?;
    let recv = call.receiver()?;
    Some((call, recv))
}

/// `node.each_node(:any_str).any?(&:heredoc?)`.
fn has_heredoc(node: &Node) -> bool {
    !helpers::heredoc::heredoc_starts(node).is_empty()
}

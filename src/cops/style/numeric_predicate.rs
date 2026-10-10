use crate::prelude::*;

pub const NAME: &str = "Style/NumericPredicate";
pub static COP: NumericPredicate = NumericPredicate;
pub struct NumericPredicate;

/// REPLACEMENTS: predicate <-> operator.
const REPLACEMENTS: &[(&str, &str)] = &[("zero?", "=="), ("positive?", ">"), ("negative?", "<")];

fn is_zero(node: &Node) -> bool {
    node.as_integer_node().is_some() && int_literal_value(node) == Some(0)
}

fn allowed(cx: &Cx, name: &str) -> bool {
    helpers::allowed::allowed_method(cx.cfg, name) || helpers::allowed::matches_allowed_pattern(cx.cfg, name)
}

impl Cop for NumericPredicate {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "==" | ">" | "<" | "positive?" | "negative?" | "zero?") {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let args = send_args(&call);
        let predicate_style = cx.cfg.str("EnforcedStyle") != Some("comparison");
        let replacement = if predicate_style {
            if args.len() != 1 || !matches!(name, "==" | ">" | "<") {
                return;
            }
            let arg = args.into_iter().next().unwrap();
            let (numeric, op) = if is_zero(&arg) && recv.as_global_variable_read_node().is_none() {
                (recv, name)
            } else if is_zero(&recv) && arg.as_global_variable_read_node().is_none() {
                (arg, invert(name))
            } else {
                return;
            };
            if !replacement_supported(cx, op) {
                return;
            }
            let pred = REPLACEMENTS.iter().find(|(_, o)| *o == op).unwrap().0;
            let src = node_text(&numeric);
            let parens = numeric.as_call_node().is_some_and(|c| {
                !is_csend(&c)
                    && helpers::node_types::is_binary_operation(&c)
                    && c.opening_loc().is_none_or(|o| o.as_slice() != b"(")
            });
            if parens { format!("({src}).{pred}") } else { format!("{src}.{pred}") }
        } else {
            if !args.is_empty() {
                return;
            }
            let Some(&(_, op)) = REPLACEMENTS.iter().find(|(p, _)| *p == name) else { return };
            if !replacement_supported(cx, op) {
                return;
            }
            let negated = helpers::parser_parent::parser_parent(cx.ancestors)
                .and_then(|p| p.as_call_node())
                .is_some_and(|p| !is_csend(&p) && method_name(&p) == "!");
            let src = node_text(&recv);
            if negated { format!("({src} {op} 0)") } else { format!("{src} {op} 0") }
        };

        if allowed(cx, name) || ancestor_names(cx.ancestors).iter().any(|n| allowed(cx, n)) {
            return;
        }
        let (start, end) = send_range(&call);
        let msg = format!("Use `{replacement}` instead of `{}`.", &cx.src.text[start..end]);
        cx.add_fix(start, end, &msg, |c| c.replace(start, end, &replacement));
    }
}

/// `positive?` / `negative?` need Ruby 2.3.
fn replacement_supported(cx: &Cx, op: &str) -> bool {
    !matches!(op, ">" | "<") || cx.config.target_ruby >= 2.3
}

fn invert(op: &str) -> &str {
    match op {
        ">" => "<",
        "<" => ">",
        o => o,
    }
}

/// Method names of the `send` and `any_block` ancestors.
fn ancestor_names<'pr>(ancestors: &[Node<'pr>]) -> Vec<&'pr str> {
    let mut out = Vec::new();
    for (i, a) in ancestors.iter().enumerate() {
        if let Some(c) = a.as_call_node() {
            let in_block = ancestors.get(i + 1).is_some_and(|n| n.as_block_node().is_some());
            let has_block = c.block().is_some_and(|b| b.as_block_node().is_some());
            // The send itself is an ancestor unless we are inside its block; the block always is.
            if (!in_block && !is_csend(&c)) || has_block {
                out.push(method_name(&c));
            }
        } else if a.as_lambda_node().is_some() {
            out.push("lambda");
        } else if (a.as_super_node().is_some_and(|s| s.block().is_some_and(|b| b.as_block_node().is_some())))
            || a.as_forwarding_super_node().is_some_and(|s| s.block().is_some())
        {
            out.push("super");
        }
    }
    out
}

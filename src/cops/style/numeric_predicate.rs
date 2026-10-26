use crate::prelude::*;

pub const NAME: &str = "Style/NumericPredicate";
pub static COP: NumericPredicate = NumericPredicate;
pub struct NumericPredicate;

/// REPLACEMENTS: predicate <-> operator.
const REPLACEMENTS: &[(&str, &str)] = &[("zero?", "=="), ("positive?", ">"), ("negative?", "<")];

fn is_zero(node: &Node) -> bool {
    int_literal_value(node) == Some(0)
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
                    && helpers::node_types::is_operator_method(method_name(&c))
                    && c.opening_loc().is_none_or(|o| o.as_slice() != b"(")
            });
            if parens { format!("({src}).{pred}") } else { format!("{src}.{pred}") }
        } else {
            if !args.is_empty() {
                return;
            }
            let Some(&(_, op)) = REPLACEMENTS.iter().find(|(p, _)| *p == name) else { return };
            // comparison_requires_parentheses?
            let (start, end) = send_range(&call);
            let parens = helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_call_node()).is_some_and(|p| {
                let is_receiver = p.receiver().is_some_and(|r| {
                    let l = r.location();
                    (l.start_offset(), l.end_offset()) == (start, end)
                });
                is_receiver || (helpers::node_types::is_operator_method(method_name(&p)) && !matches!(method_name(&p), "[]" | "[]="))
            });
            let src = node_text(&recv);
            if parens { format!("({src} {op} 0)") } else { format!("{src} {op} 0") }
        };

        if handled_by_bitwise_predicate(cx, &call) {
            return;
        }
        if allowed(cx, name) || ancestor_names(cx.ancestors).iter().any(|n| allowed(cx, n)) {
            return;
        }
        let (start, end) = send_range(&call);
        let msg = format!("Use `{replacement}` instead of `{}`.", &cx.src.text[start..end]);
        cx.add_fix(start, end, &msg, |c| c.replace(start, end, &replacement));
    }
}

/// `handled_by_bitwise_predicate?`: `(a & b).zero?`, `(a & b) > 0` and friends are left to
/// Style/BitwisePredicate when it is enabled and supported.
fn handled_by_bitwise_predicate(cx: &Cx, call: &ruby_prism::CallNode) -> bool {
    let Some(recv) = call.receiver() else { return false };
    let recv = match recv.as_parentheses_node() {
        Some(p) => {
            let Some(body) = p.body() else { return false };
            match body.as_statements_node().map(|s| s.body()) {
                Some(b) if b.len() == 1 => b.iter().next().unwrap(),
                Some(_) => return false,
                None => body,
            }
        }
        None => recv,
    };
    let bitwise_and = recv
        .as_call_node()
        .is_some_and(|c| !is_csend(&c) && method_name(&c) == "&" && c.receiver().is_some() && send_args(&c).len() == 1);
    let args = send_args(call);
    let candidate = bitwise_and
        && match method_name(call) {
            "positive?" | "zero?" => args.is_empty(),
            ">" | "==" => args.len() == 1 && args[0].as_integer_node().is_some_and(|_| is_zero(&args[0])),
            _ => false,
        };
    candidate && cx.config.for_cop("Style/BitwisePredicate").truthy("Enabled") && cx.config.target_ruby >= 2.5
}

/// `positive?` / `negative?` need Ruby 2.3. Only the predicate style's operator can be `>`/`<`;
/// the comparison style's is the predicate name, which is always supported.
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

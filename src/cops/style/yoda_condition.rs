use crate::helpers::literal_node::recursive_basic_literal;
use crate::prelude::*;

pub const NAME: &str = "Style/YodaCondition";
pub static COP: YodaCondition = YodaCondition;
pub struct YodaCondition;

/// `recursive_literal? || const_type?`. rubocop-ast `recursive_literal?` only differs from
/// `recursive_basic_literal?` on `regopt`, which the helper already treats as literal.
fn constant_portion(node: &Node) -> bool {
    recursive_basic_literal(node) || node.as_constant_read_node().is_some() || node.as_constant_path_node().is_some()
}

fn interpolation(node: &Node) -> bool {
    is_parser_dstr_exact(node) || node.as_interpolated_regular_expression_node().is_some()
}

impl Cop for YodaCondition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let op = method_name(&call);
        if is_csend(&call) || !matches!(op, "==" | "!=" | "<" | ">" | "<=" | ">=") {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("forbid_for_all_comparison_operators");
        let equality_only =
            matches!(style, "forbid_for_equality_operators_only" | "require_for_equality_operators_only");
        if equality_only && !matches!(op, "==" | "!=") {
            return;
        }
        let Some(lhs) = call.receiver() else { return };
        let args = send_args(&call);
        // file_constant_equal_program_name?
        if node_text(&lhs) == "__FILE__"
            && matches!(op, "==" | "!=")
            && args.len() == 1
            && args[0]
                .as_global_variable_read_node()
                .is_some_and(|g| matches!(g.name().as_slice(), b"$0" | b"$PROGRAM_NAME"))
        {
            return;
        }
        let Some(rhs) = args.first() else { return };
        let (cl, cr) = (constant_portion(&lhs), constant_portion(rhs));
        if cl == cr || interpolation(&lhs) {
            return;
        }
        let enforce = matches!(style, "require_for_all_comparison_operators" | "require_for_equality_operators_only");
        if if enforce { cl } else { cr } {
            return;
        }
        let rev = match op {
            "<" => ">",
            "<=" => ">=",
            ">" => "<",
            ">=" => "<=",
            o => o,
        };
        let (s, e) = send_range(&call);
        let fixed = format!("{} {rev} {}", node_text(rhs), node_text(&lhs));
        let msg = format!("Reverse the order of the operands `{}`.", &cx.src.text[s..e]);
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &fixed));
    }
}

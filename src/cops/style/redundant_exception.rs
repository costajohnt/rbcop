use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Style/RedundantException";
pub static COP: RedundantException = RedundantException;
pub struct RedundantException;

impl Cop for RedundantException {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let command = method_name(&call);
        if call.receiver().is_some() || is_csend(&call) || !matches!(command, "raise" | "fail") {
            return;
        }
        let args = send_args(&call);
        let (s, e) = send_range(&call);
        match args.as_slice() {
            // (send nil? ${:raise :fail} (const {nil? cbase} :RuntimeError) $_)
            [exc, message] if is_top_const(exc, "RuntimeError") => {
                if message.as_nil_node().is_some() {
                    return;
                }
                let arg = message_source(message);
                let arg = if call.opening_loc().is_some() { format!("({arg})") } else { format!(" {arg}") };
                let fix = format!("{command}{arg}");
                cx.add_fix(s, e, "Redundant `RuntimeError` argument can be removed.", |c| c.replace(s, e, &fix));
            }
            // (send nil? {:raise :fail} $(send (const {nil? cbase} :RuntimeError) :new $_))
            [new_call] => {
                let Some(new) = new_call.as_call_node() else { return };
                if is_csend(&new)
                    || method_name(&new) != "new"
                    || !new.receiver().is_some_and(|r| is_top_const(&r, "RuntimeError"))
                    || new.block().is_some_and(|b| b.as_block_node().is_some())
                {
                    return;
                }
                let new_args = send_args(&new);
                let [message] = new_args.as_slice() else { return };
                if message.as_nil_node().is_some() {
                    return;
                }
                let fix = message_source(message);
                let (ns, ne) = send_range(&new);
                cx.add_fix(s, e, "Redundant `RuntimeError.new` call can be replaced with just the message.", |c| {
                    c.replace(ns, ne, &fix)
                });
            }
            _ => {}
        }
    }
}

fn message_source(message: &Node) -> String {
    let src = node_text(message);
    match kind::of(message) {
        kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE | kind::X_STRING_NODE | kind::INTERPOLATED_X_STRING_NODE => {
            src.to_string()
        }
        _ if require_parentheses(message) => format!("({src}).to_s"),
        _ => format!("{src}.to_s"),
    }
}

/// `require_parentheses?(message)`
fn require_parentheses(message: &Node) -> bool {
    if let Some(c) = message.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none())) {
        let name = method_name(&c);
        let has_args = c.arguments().is_some() || c.block().is_some();
        let parenthesized = c.opening_loc().is_some_and(|o| o.as_slice() == b"(");
        return name != "[]" && (helpers::node_types::is_operator_method(name) || (has_args && !parenthesized));
    }
    // basic_literal?, variable?, const, self, begin, array
    !matches!(
        kind::of(message),
        kind::STRING_NODE
            | kind::SOURCE_FILE_NODE
            | kind::INTEGER_NODE
            | kind::SOURCE_LINE_NODE
            | kind::FLOAT_NODE
            | kind::SYMBOL_NODE
            | kind::TRUE_NODE
            | kind::FALSE_NODE
            | kind::NIL_NODE
            | kind::INSTANCE_VARIABLE_READ_NODE
            | kind::GLOBAL_VARIABLE_READ_NODE
            | kind::CLASS_VARIABLE_READ_NODE
            | kind::LOCAL_VARIABLE_READ_NODE
            | kind::IT_LOCAL_VARIABLE_READ_NODE
            | kind::CONSTANT_READ_NODE
            | kind::CONSTANT_PATH_NODE
            | kind::SELF_NODE
            | kind::PARENTHESES_NODE
            | kind::BEGIN_NODE
            | kind::ARRAY_NODE
    )
}
